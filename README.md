Hame Energy Toolkit
===================

A collections of tools for Hame energy storage.
Reads, writes and transforms data from Hame energy storage devices, like the B2500 series.


**Note**: Currently this in a very early development stage, you should instead look into:

- [hm2mqtt](https://github.com/tomquist/hm2mqtt)
- [hame-relay](https://github.com/tomquist/hame-relay)

<img width="1784" alt="Image" src="https://github.com/user-attachments/assets/4e10d86c-80af-42e5-836f-1219dc566303" />


## Setup

`hmtk` currently requires direct access to an MQTT broker the device is connected to.
To configure a custom MQTT broker on the device, [this web-utility](https://tomquist.github.io/hame-relay/b2500.html)
can be used.

**Note**: configuring a custom MQTT broker, without setting up a
[relay](https://github.com/tomquist/hame-relay/), will disable web/wifi access of the app.

For local development, I recommend setting up a `.env` file, containing the MQTT broker settings:

```sh
HMTK_MQTT_HOST=127.0.0.1
HMTK_MQTT_PORT=1883
HMTK_MQTT_USERNAME=admin
HMTK_MQTT_PASSWORD=password
```

## Querying Metrics

Metrics can be collected via cli, currently supported output formats are JSON and the influx line protocol.

```sh
$ htmk \
  --mqtt-host 127.0.0.1 --mqtt-port 1883 --mqtt-username <user> --mqtt-password <password> \
  --device --mac <mac> --type <type> \
  query (--influx|--json)
```

Example Output:

```json
{
  "timestamp": 1745745900,
  "solar1": {
    "charging": true,
    "pass_through": false,
    "power": 84
  },
  "solar2": {
    "charging": true,
    "pass_through": false,
    "power": 86
  },
  "output1": {
    "power": 1,
    "active": true
  },
  "output2": {
    "power": 0,
    "active": true
  },
  "temperature": {
    "min": 21,
    "max": 21
  },
  "battery": {
    "charge": 53,
    "capacity": 1187,
    "output_threshold": 300,
    "discharge_depth": 80,
    "internal": {
      "charging": false,
      "discharging": true,
      "discharge_depth": false,
      "undervoltage": false
    }
  },
  "scene": "day"
}
```

### Telegraf / InfluxDB

Collection via [telegraf](https://github.com/influxdata/telegraf) can be easily setup using the exec plugin:

```toml
[[inputs.exec]]
  commands = [
    'hmtk --device --mac <mac> --type <type> query --influx',
  ]

  environment = [
    "HMTK_MQTT_HOST=127.0.0.1",
    "HMTK_MQTT_PORT=1883",
    "HMTK_MQTT_USERNAME=admin",
    "HMTK_MQTT_PASSWORD=password",
  ]

  timeout = "10s"
  data_format = "influx"
```


## Resources:

- [B2500 Communication Protocol (DE)](https://forum.iobroker.net/assets/uploads/files/1700144946056-b2500-mqtt-communication-protocol-de.pdf)
- [hm2mqtt](https://github.com/tomquist/hm2mqtt)
- [hame-relay](https://github.com/tomquist/hame-relay)
- [hm2500pub](https://github.com/noone2k/hm2500pub) (especially interesting for the BT protocol)
- [Photovoltaikforum](https://www.photovoltaikforum.com/thread/232408)
- [Hamedata MQTT](https://eu.hamedata.com/ems/mqtt/index.html?version=2)

