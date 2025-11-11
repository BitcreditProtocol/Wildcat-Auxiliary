use anyhow::anyhow;
use borsh::io::{Error as BorshError, ErrorKind, Read, Write};
use std::str::FromStr;

pub mod challenge;
pub mod email;
pub mod signature;
pub mod wire;

pub type TStamp = chrono::DateTime<chrono::Utc>;

pub fn now() -> TStamp {
    chrono::Utc::now()
}

pub fn serialize_as_str<T>(t: &T, writer: &mut impl Write) -> Result<(), BorshError>
where
    T: std::fmt::Display,
{
    let stringified = t.to_string();
    borsh::BorshSerialize::serialize(&stringified, writer)?;
    Ok(())
}

pub fn deserialize_as_str<T>(reader: &mut impl Read) -> Result<T, BorshError>
where
    T: FromStr,
    <T as FromStr>::Err: Into<Box<dyn std::error::Error + Send + Sync>>,
{
    let stringified: String = borsh::BorshDeserialize::deserialize_reader(reader)?;
    let t = T::from_str(&stringified).map_err(|e| BorshError::new(ErrorKind::InvalidInput, e))?;
    Ok(t)
}

pub fn serialize_tstamp_as_u64(t: &TStamp, writer: &mut impl Write) -> Result<(), BorshError> {
    let v: u64 = t.timestamp() as u64;
    borsh::BorshSerialize::serialize(&v, writer)?;
    Ok(())
}

pub fn deserialize_tstamp_as_u64(reader: &mut impl Read) -> Result<TStamp, BorshError> {
    let v: u64 = borsh::BorshDeserialize::deserialize_reader(reader)?;
    let tstamp: TStamp = TStamp::from_timestamp_secs(v as i64).ok_or(BorshError::new(
        ErrorKind::InvalidInput,
        anyhow!("invalid timestamp"),
    ))?;
    Ok(tstamp)
}
