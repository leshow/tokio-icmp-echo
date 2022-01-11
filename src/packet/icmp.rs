use std::io::Write;

pub const HEADER_SIZE: usize = 8;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("invalid size")]
    InvalidSize,
    #[error("invalid packet")]
    InvalidPacket,
}

pub struct IcmpV4;
pub struct IcmpV6;

pub trait Proto {
    const ECHO_REQUEST_TYPE: u8;
    const ECHO_REQUEST_CODE: u8;
    const ECHO_REPLY_TYPE: u8;
    const ECHO_REPLY_CODE: u8;
}

impl Proto for IcmpV4 {
    const ECHO_REQUEST_TYPE: u8 = 8;
    const ECHO_REQUEST_CODE: u8 = 0;
    const ECHO_REPLY_TYPE: u8 = 0;
    const ECHO_REPLY_CODE: u8 = 0;
}

impl Proto for IcmpV6 {
    const ECHO_REQUEST_TYPE: u8 = 128;
    const ECHO_REQUEST_CODE: u8 = 0;
    const ECHO_REPLY_TYPE: u8 = 129;
    const ECHO_REPLY_CODE: u8 = 0;
}

#[derive(Debug)]
pub struct EchoRequest<'a> {
    pub ident: u16,
    pub seq_cnt: u16,
    pub payload: &'a [u8],
}

impl<'a> EchoRequest<'a> {
    pub fn encode<P: Proto>(&self, buffer: &mut [u8]) -> Result<(), Error> {
        buffer[0] = P::ECHO_REQUEST_TYPE;
        buffer[1] = P::ECHO_REQUEST_CODE;

        buffer[4..=5].copy_from_slice(&u16::to_be_bytes(self.ident));
        buffer[6..=7].copy_from_slice(&u16::to_be_bytes(self.seq_cnt));

        if (&mut buffer[8..]).write(self.payload).is_err() {
            return Err(Error::InvalidSize);
        }

        write_checksum(buffer);
        Ok(())
    }
}

#[derive(Debug)]
pub struct EchoReply<'a> {
    pub ident: u16,
    pub seq_cnt: u16,
    pub payload: &'a [u8],
}

impl<'a> EchoReply<'a> {
    pub fn decode<P: Proto>(buffer: &'a [u8]) -> Result<Self, Error> {
        if buffer.as_ref().len() < HEADER_SIZE {
            return Err(Error::InvalidSize);
        }

        let type_ = buffer[0];
        let code = buffer[1];
        if type_ != P::ECHO_REPLY_TYPE && code != P::ECHO_REPLY_CODE {
            return Err(Error::InvalidPacket);
        }

        let ident = u16::from_be_bytes([buffer[4], buffer[5]]);
        let seq_cnt = u16::from_be_bytes([buffer[6], buffer[7]]);
        let payload = &buffer[HEADER_SIZE..];

        Ok(EchoReply {
            ident,
            seq_cnt,
            payload,
        })
    }
}

fn write_checksum(buffer: &mut [u8]) {
    let mut sum = 0u32;
    for word in buffer.chunks(2) {
        let mut part = u16::from(word[0]) << 8;
        if word.len() > 1 {
            part += u16::from(word[1]);
        }
        sum = sum.wrapping_add(u32::from(part));
    }

    while (sum >> 16) > 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }

    let sum = !sum as u16;

    buffer[2] = (sum >> 8) as u8;
    buffer[3] = (sum & 0xff) as u8;
}
