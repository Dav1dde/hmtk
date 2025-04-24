use std::fmt::{self, Write as _};
use std::time::{Duration, SystemTime};

macro_rules! wrt {
    ($dst:expr, $($arg:tt)*) => {
        write!($dst, $($arg)*).expect("writing to a string never fails");
    };
}

pub struct Measurement<'a> {
    name: &'a str,
    tags: String,
    fields: String,
    timestamp: Option<SystemTime>,
}

impl<'a> Measurement<'a> {
    /// Creates a new measurement named `name`.
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            tags: String::new(),
            fields: String::new(),
            timestamp: None,
        }
    }

    /// Appends a tag to the measurement.
    pub fn tag(&mut self, key: &str, value: &str) -> &mut Self {
        if !value.is_empty() {
            if !self.tags.is_empty() {
                self.tags.push(',');
            }
            wrt!(&mut self.tags, "{key}={value}");
        }
        self
    }

    /// Appends a field to the measurement.
    pub fn field<T: InfluxValue>(&mut self, key: &str, value: T) -> &mut Self {
        if !self.fields.is_empty() {
            self.fields.push(',');
        }
        wrt!(&mut self.fields, "{key}=");
        value.write_to(&mut self.fields);
        self
    }

    pub fn timestamp(&mut self, timestamp: SystemTime) -> &mut Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Appends the measurement to the `sink`.
    ///
    /// Unlike the `Display` implementation, this also adds a `\n`
    /// to the end of the measurement.
    pub fn write_to(&self, sink: &mut String) {
        wrt!(sink, "{self}\n");
    }
}

impl fmt::Display for Measurement<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name)?;
        if !self.tags.is_empty() {
            f.write_str(",")?;
            f.write_str(&self.tags)?;
        }
        f.write_str(" ")?;
        f.write_str(&self.fields)?;
        if let Some(timestamp) = self.timestamp {
            let duration = timestamp
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_nanos();

            write!(f, " {duration}")?;
        }

        Ok(())
    }
}

mod ඞ {
    use std::fmt::Write;

    pub trait InfluxValue {
        fn write_to(&self, sink: &mut String);
    }

    impl InfluxValue for &str {
        fn write_to(&self, sink: &mut String) {
            wrt!(sink, "{self:?}");
        }
    }

    macro_rules! impl_display {
        ($($ty:ty),*) => {
            $(impl InfluxValue for $ty {
                fn write_to(&self, sink: &mut String) {
                    wrt!(sink, "{self}");
                }
            })*
        };
    }
    impl_display!(bool, f32, f64);

    macro_rules! impl_signed {
        ($($ty:ty),*) => {
            $(impl InfluxValue for $ty {
                fn write_to(&self, sink: &mut String) {
                    wrt!(sink, "{self}i");
                }
            })*
        };
    }
    impl_signed!(i8, i16, i32, i64);

    macro_rules! impl_unsigned {
        ($($ty:ty),*) => {
            $(impl InfluxValue for $ty {
                fn write_to(&self, sink: &mut String) {
                    wrt!(sink, "{self}u");
                }
            })*
        };
    }
    impl_unsigned!(u8, u16, u32, u64);
}
use self::ඞ::InfluxValue;
