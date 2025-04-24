macro_rules! impl_unit {
    ($name:ident, $ty:ty) => {
        #[derive(Default, Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub $ty);

        impl std::str::FromStr for $name {
            type Err = <$ty as std::str::FromStr>::Err;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.parse().map(Self)
            }
        }
    };
}

impl_unit!(Watt, u32);
impl_unit!(WattHours, u32);
impl_unit!(Celsius, i32);
impl_unit!(Percentage, u8);
