use core::fmt;
use std::{
    collections::BTreeMap,
    io::ErrorKind,
    str::FromStr,
    time::{Duration, SystemTime},
};

use futures::FutureExt;
use rumqttc::{
    AsyncClient, ConnectionError, Event, EventLoop, MqttOptions, Outgoing, Packet, QoS, StateError,
};
use serde::Serialize;
use tokio::sync::watch;

use crate::{
    mqtt::{Error, InvalidStatus, Result},
    units::{Celsius, Percentage, Watt, WattHours},
};

#[derive(Debug, Clone)]
pub struct DeviceOptions {
    pub ty: String,
    pub mac: String,
}

impl DeviceOptions {
    fn data_topic(&self) -> String {
        format!("hame_energy/{}/device/{}/ctrl", self.ty, self.mac)
    }

    fn control_topic(&self) -> String {
        format!("hame_energy/{}/App/{}/ctrl", self.ty, self.mac)
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct DeviceInfo {
    #[serde(serialize_with = "ser_system_time_secs")]
    pub timestamp: SystemTime,
    pub solar1: SolarInfo,
    pub solar2: SolarInfo,
    pub output1: OutputInfo,
    pub output2: OutputInfo,
    pub temperature: TemperatureInfo,
    pub battery: BatteryInfo,
    pub scene: Scene,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct SolarInfo {
    pub charging: bool,
    pub pass_through: bool,
    pub power: Watt,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct OutputInfo {
    pub power: Watt,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct TemperatureInfo {
    pub min: Celsius,
    pub max: Celsius,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BatteryInfo {
    pub charge: Percentage,
    pub capacity: WattHours,
    pub output_threshold: Watt,
    pub discharge_depth: Percentage,
    pub internal: BatteryCellInfo,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BatteryCellInfo {
    pub charging: bool,
    pub discharging: bool,
    pub discharge_depth: bool,
    pub undervoltage: bool,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scene {
    Day,
    Night,
    Dusk,
}

impl Scene {
    pub fn as_str(self) -> &'static str {
        match self {
            Scene::Day => "day",
            Scene::Night => "night",
            Scene::Dusk => "dusk",
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid Scene")]
pub struct InvalidSceneError;

impl FromStr for Scene {
    type Err = InvalidSceneError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s {
            "0" => Scene::Day,
            "1" => Scene::Night,
            "2" => Scene::Dusk,
            _ => return Err(InvalidSceneError),
        })
    }
}

impl From<&Measurement<RawDeviceInfo>> for DeviceInfo {
    fn from(value: &Measurement<RawDeviceInfo>) -> Self {
        macro_rules! bit {
            ($value:expr, $bit:literal) => {
                ($value >> $bit) & 0b01 == 1
            };
        }

        let timestamp = value.time;
        let value = value.data.as_ref().expect("valid measurement");
        DeviceInfo {
            timestamp,
            solar1: SolarInfo {
                charging: bit!(value.p1, 0),
                pass_through: bit!(value.p1, 1),
                power: value.w1,
            },
            solar2: SolarInfo {
                charging: bit!(value.p2, 0),
                pass_through: bit!(value.p2, 1),
                power: value.w2,
            },
            output1: OutputInfo {
                power: value.g1,
                active: bit!(value.o1, 0),
            },
            output2: OutputInfo {
                power: value.g2,
                active: bit!(value.o2, 0),
            },
            temperature: TemperatureInfo {
                min: value.tl,
                max: value.th,
            },
            battery: BatteryInfo {
                charge: value.pe,
                capacity: value.kn,
                output_threshold: value.lv,
                discharge_depth: value.r#do,
                internal: BatteryCellInfo {
                    charging: bit!(value.l0, 0),
                    discharging: bit!(value.l0, 1),
                    discharge_depth: bit!(value.l0, 2),
                    undervoltage: bit!(value.l0, 3),
                },
            },
            scene: value.cj,
        }
    }
}

/// A Hame energy storage device as represented in MQTT.
#[derive(Debug, Clone)]
pub struct Device {
    client: AsyncClient,
    options: DeviceOptions,
    device_info: watch::Receiver<Measurement<RawDeviceInfo>>,
}

impl Device {
    pub fn new(mqtt: MqttOptions, device: DeviceOptions) -> Result<(Self, DeviceLoop)> {
        let (client, ev) = AsyncClient::new(mqtt, 10);

        client
            .try_subscribe(device.data_topic(), QoS::AtMostOnce)
            .expect("initial subscribe to succeed");

        let (device_info_tx, device_info_rx) = watch::channel(Default::default());

        let dev = Self {
            client,
            options: device,
            device_info: device_info_rx,
        };
        let ev = DeviceLoop {
            ev,
            disconnect: false,
            device_info: device_info_tx,
        };

        Ok((dev, ev))
    }

    pub fn options(&self) -> &DeviceOptions {
        &self.options
    }

    // TODO: there should be a variant which forces a refresh, async refreshes or just reads the
    // current values.
    pub async fn device_info(&mut self) -> Result<DeviceInfo> {
        self.client
            .publish(
                self.options.control_topic(),
                QoS::AtLeastOnce,
                false,
                "cd=1",
            )
            .await?;

        let _ = self.device_info.changed().await;
        let value = self.device_info.borrow_and_update();

        Ok(DeviceInfo::from(&*value))
    }

    /// Disconnects the client from the broker.
    ///
    /// This disconnects the device loop from the broker, rendering all instances of this
    /// client disconnected and no longer functional.
    pub async fn disconnect(&mut self) -> Result<()> {
        self.client.disconnect().await?;
        Ok(())
    }
}

pub struct DeviceLoop {
    ev: EventLoop,
    disconnect: bool,
    device_info: watch::Sender<Measurement<RawDeviceInfo>>,
}

impl IntoFuture for DeviceLoop {
    type Output = Result<()>;
    type IntoFuture = futures::future::BoxFuture<'static, Self::Output>;

    fn into_future(self) -> Self::IntoFuture {
        self.run().boxed()
    }
}

impl DeviceLoop {
    async fn run(mut self) -> Result<()> {
        // TODO: error handling
        loop {
            match self.ev.poll().await {
                Ok(Event::Incoming(Packet::Publish(message))) => {
                    tracing::debug!("received on {} value {:?}", message.topic, message.payload);

                    // TODO: filter topic
                    let message = Message::parse(message.payload).unwrap();
                    let device_info = RawDeviceInfo::try_from(&message).unwrap();
                    let Ok(()) = self.device_info.send(Measurement::new(device_info)) else {
                        tracing::debug!("sender disconnected, exiting event loop");
                        return Ok(());
                    };
                }
                Ok(Event::Incoming(packet)) => {
                    tracing::trace!("received {packet:?}");
                }
                Ok(Event::Outgoing(Outgoing::Disconnect)) => {
                    tracing::debug!("client wants to disconnect");
                    self.disconnect = true;
                }
                Ok(Event::Outgoing(out)) => {
                    tracing::trace!("sent: {out:?}");
                }
                Err(ConnectionError::MqttState(StateError::Io(io)))
                    if io.kind() == ErrorKind::ConnectionAborted && self.disconnect =>
                {
                    // Client sent a disconnect and the connection is now closed.
                    return Ok(());
                }
                Err(err) => {
                    tracing::warn!("connection error: {err}");
                }
            }
        }
    }
}

struct Message {
    payload: BTreeMap<String, String>,
}

impl Message {
    pub fn parse(raw_message: bytes::Bytes) -> Result<Self> {
        let message = std::str::from_utf8(&raw_message)
            .map_err(|_| InvalidStatus::InvalidFormat(raw_message.clone()))?
            .to_owned();

        let mut payload = BTreeMap::new();

        for part in message.trim().split(',') {
            let Some((key, value)) = part.split_once('=') else {
                return Err(InvalidStatus::InvalidFormat(raw_message).into());
            };

            payload.insert(key.to_owned(), value.to_owned());
        }

        Ok(Message { payload })
    }

    pub fn get_value<T: FromStr>(&self, name: &str) -> Result<Option<T>, T::Err> {
        self.payload
            .get(name)
            .map(|value| value.parse())
            .transpose()
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut s = f.debug_struct("Message");
        for (name, value) in self.payload.iter() {
            s.field(name, value);
        }
        s.finish()
    }
}

#[derive(Debug)]
struct Measurement<T> {
    pub time: SystemTime,
    pub data: Option<T>,
}

impl<T> Measurement<T> {
    pub fn new(data: T) -> Self {
        Self {
            time: SystemTime::now(),
            data: Some(data),
        }
    }
}

impl<T> Default for Measurement<T> {
    fn default() -> Self {
        Self {
            time: SystemTime::UNIX_EPOCH,
            data: None,
        }
    }
}

macro_rules! message {
    (struct $name:ident {
        $(
            $(#[$attr:meta])*
            $field:ident: $ty:ty,
        )*
    }) => {
        #[derive(Debug, Clone)]
        struct $name {
            $(
                $(#[$attr])*
                $field: $ty,
            )*
        }

        impl TryFrom<&Message> for $name {
            type Error = Error;

            fn try_from(message: &Message) -> Result<Self, Self::Error> {
                Ok(Self {
                    $(
                        $field: match stringify!($field).trim_start_matches("r#") {
                            field => message
                                .get_value(field)
                                .map_err(|err| InvalidStatus::InvalidField(field, Box::new(err)))?
                                .ok_or(InvalidStatus::MissingField(field))?,
                        },
                    )*

                })
            }
        }
    };
}

message! {
    struct RawDeviceInfo {
        /// Solar 1: Input Status.
        p1: u8,
        /// Solar 2: Input Status.
        p2: u8,
        /// Solar 1: Input Power.
        w1: Watt,
        /// Solar 2: Input Power.
        w2: Watt,
        /// Battery Percentage.
        pe: Percentage,

        /// Output 1: State.
        o1: u8,
        /// Output 2: State.
        o2: u8,
        /// Discharge Depth.
        r#do: Percentage,
        /// Battery Output Threshold.
        lv: Watt,
        /// Scene
        cj: Scene,
        /// Battery Capacity.
        kn: WattHours,
        /// Output 1: Power.
        g1: Watt,
        /// Output 2: Power.
        g2: Watt,

        /// Temperature Min.
        tl: Celsius,
        /// Temperature Max.
        th: Celsius,

        /// Host Battery Status.
        l0: u8,
    }
}

fn ser_system_time_secs<S: serde::Serializer>(
    value: &SystemTime,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let seconds = value
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs();
    serializer.serialize_u64(seconds)
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;

    use super::*;

    #[test]
    fn test_message_device_info() {
        // Payload obtained by sending `cd=01`.
        let payload = b"p1=1,p2=1,w1=23,w2=23,pe=99,vv=220,sv=12,cs=0,cd=0,am=0,o1=1,o2=1,do=80,lv=200,cj=2,kn=2217,g1=1,g2=0,b1=0,b2=0,md=0,d1=1,e1=0:0,f1=23:59,h1=200,d2=0,e2=0:0,f2=0:0,h2=600,d3=0,e3=0:0,f3=0:0,h3=0,sg=0,sp=80,st=0,tl=27,th=27,tc=0,tf=0,fc=202310231502,id=5,a0=99,a1=0,a2=0,l0=1,l1=0,c0=255,c1=0,bc=2025,bs=329,pt=3332,it=1518,m0=0,m1=0,m2=0,m3=1,d4=0,e4=0:0,f4=24:0,h4=80,d5=0,e5=0:0,f5=24:0,h5=80,lmo=1830,lmi=272,lmf=1";
        let payload = Bytes::from_static(payload);

        let message = Message::parse(payload).unwrap();
        let message = RawDeviceInfo::try_from(&message).unwrap();
        insta::assert_debug_snapshot!(message, @r###"
        RawDeviceInfo {
            p1: 1,
            p2: 1,
            w1: Watt(
                23,
            ),
            w2: Watt(
                23,
            ),
            pe: Percentage(
                99,
            ),
            o1: 1,
            o2: 1,
            do: Percentage(
                80,
            ),
            lv: Watt(
                200,
            ),
            cj: Dusk,
            kn: WattHours(
                2217,
            ),
            g1: Watt(
                1,
            ),
            g2: Watt(
                0,
            ),
            tl: Celsius(
                27,
            ),
            th: Celsius(
                27,
            ),
            l0: 1,
        }
        "###);
    }

    #[test]
    fn test_message_battery_data() {
        // Payload obtained by sending `cd=16`.
        let payload = b"p1=0,p2=0,m1=36957,m2=37457,c1=1,c2=0,w1=0,w2=0,e1=1,e2=1,o1=2,o2=2,i1=39732,i2=39482,c3=3692,c4=3580,g1=116,g2=112,sg=0,sp=80,st=0,ps=3,bb=56,bv=46463,bc=1521,sb=0,sv=0,sc=0,lb=0,lv=0,lc=0";
        let payload = Bytes::from_static(payload);

        let message = Message::parse(payload).unwrap();
        insta::assert_debug_snapshot!(message, @r###"
            Message {
                bb: "56",
                bc: "1521",
                bv: "46463",
                c1: "1",
                c2: "0",
                c3: "3692",
                c4: "3580",
                e1: "1",
                e2: "1",
                g1: "116",
                g2: "112",
                i1: "39732",
                i2: "39482",
                lb: "0",
                lc: "0",
                lv: "0",
                m1: "36957",
                m2: "37457",
                o1: "2",
                o2: "2",
                p1: "0",
                p2: "0",
                ps: "3",
                sb: "0",
                sc: "0",
                sg: "0",
                sp: "80",
                st: "0",
                sv: "0",
                w1: "0",
                w2: "0",
            }
        "###);
    }
}
