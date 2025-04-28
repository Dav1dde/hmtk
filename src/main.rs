use bpaf::Bpaf;
use color_eyre::eyre::Result;
use hmtk::mqtt::DeviceOptions;
use rumqttc::MqttOptions;

#[derive(Debug, Clone, Bpaf)]
#[bpaf(options)]
struct Args {
    /// MQTT host the battery is connected to.
    #[bpaf(env("HMTK_MQTT_HOST"))]
    mqtt_host: String,
    /// Port of the MQTT server.
    #[bpaf(env("HMTK_MQTT_PORT"), fallback(1883))]
    mqtt_port: u16,
    /// MQTT client id.
    #[bpaf(env("HMTK_MQTT_CLIENT"), fallback("hmtk".to_owned()))]
    mqtt_client: String,
    #[bpaf(external, optional)]
    mqtt_credentials: Option<MqttCredentials>,

    // TODO: this could be device or credentials, to query it from the API
    #[bpaf(external)]
    device: Device,

    #[bpaf(external)]
    action: Action,
}

#[derive(Debug, Clone, Bpaf)]
struct MqttCredentials {
    /// Username used to connect to the MQTT server.
    #[bpaf(env("HMTK_MQTT_USERNAME"))]
    mqtt_username: String,
    /// Password used to connect to the MQTT server.
    #[bpaf(env("HMTK_MQTT_PASSWORD"))]
    mqtt_password: String,
}

#[derive(Debug, Clone, Bpaf)]
#[bpaf(adjacent)]
struct Device {
    /// Battery device options.
    #[expect(unused, reason = "required for bpaf")]
    device: (),
    /// The MAC of the device.
    ///
    /// For example: `9523ccae1a9b`.
    mac: String,
    /// The type of the device.
    ///
    /// For example: `HMA-1`.
    r#type: String,
}

#[derive(Debug, Clone, Bpaf)]
enum Action {
    /// Query current statistics from the battery.
    #[bpaf(command)]
    Query {
        /// Output format.
        #[bpaf(external(query_format))]
        format: QueryFormat,
    },
}

#[derive(Debug, Clone, Bpaf)]
enum QueryFormat {
    /// Outputs the current measurements as JSON.
    Json,
    /// Outputs the current measurements in InfluxDB line format.
    Influx,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = args().run();

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let mut options = MqttOptions::new(args.mqtt_client, args.mqtt_host, args.mqtt_port);
    options.set_clean_session(true);
    if let Some(MqttCredentials {
        mqtt_username,
        mqtt_password,
    }) = args.mqtt_credentials
    {
        options.set_credentials(mqtt_username, mqtt_password);
    }

    let (mut device, device_loop) = hmtk::mqtt::Device::new(
        options,
        DeviceOptions {
            ty: args.device.r#type,
            mac: args.device.mac,
        },
    )?;

    let device_loop = tokio::task::spawn(device_loop.into_future());

    match args.action {
        Action::Query { format } => query(&mut device, format),
    }
    .await?;

    device.disconnect().await?;
    device_loop.await??;

    Ok(())
}

async fn query(device: &mut hmtk::mqtt::Device, format: QueryFormat) -> Result<()> {
    let device_info = device.device_info().await?;

    let out = match format {
        QueryFormat::Json => serde_json::to_string_pretty(&device_info)?,
        QueryFormat::Influx => to_influx(device.options(), &device_info),
    };

    println!("{out}");

    Ok(())
}

fn to_influx(device: &DeviceOptions, device_info: &hmtk::mqtt::DeviceInfo) -> String {
    let mut result = String::new();

    macro_rules! measurement {
        () => {
            hmtk::influx::Measurement::new("hmtk")
                .tag("device_type", &device.ty)
                .tag("device_mac", &device.mac)
                .timestamp(device_info.timestamp)
        };
    }

    for (i, solar) in [device_info.solar1, device_info.solar2].iter().enumerate() {
        measurement!()
            .tag("solar", &(i + 1).to_string())
            .field("solar_charging", solar.charging)
            .field("solar_pass_through", solar.pass_through)
            .field("solar_power", solar.power.0)
            .write_to(&mut result);
    }

    for (i, output) in [device_info.output1, device_info.output2]
        .iter()
        .enumerate()
    {
        measurement!()
            .tag("output", &(i + 1).to_string())
            .field("output_active", output.active)
            .field("output_power", output.power.0)
            .write_to(&mut result);
    }

    measurement!()
        .field("scene", device_info.scene.as_str())
        .field("temperature_min", device_info.temperature.min.0)
        .field("temperature_max", device_info.temperature.max.0)
        .field("battery_charge", device_info.battery.charge.0)
        .field("battery_capacity", device_info.battery.capacity.0)
        .field(
            "battery_output_threshold",
            device_info.battery.output_threshold.0,
        )
        .field(
            "battery_discharge_depth",
            device_info.battery.discharge_depth.0,
        )
        .write_to(&mut result);

    measurement!()
        .tag("battery_cell", "internal")
        .field(
            "battery_cell_charging",
            device_info.battery.internal.charging,
        )
        .field(
            "battery_cell_discharging",
            device_info.battery.internal.discharging,
        )
        .field(
            "battery_cell_discharge_depth",
            device_info.battery.internal.discharge_depth,
        )
        .field(
            "battery_cell_undervoltage",
            device_info.battery.internal.undervoltage,
        )
        .write_to(&mut result);

    result
}
