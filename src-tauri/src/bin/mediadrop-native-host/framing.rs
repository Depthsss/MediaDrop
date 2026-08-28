use std::io::{self, Read, Write};

pub(crate) fn read_frame(reader: &mut impl Read, maximum: usize) -> io::Result<Option<Vec<u8>>> {
    let mut prefix = [0u8; 4];
    match reader.read(&mut prefix[..1])? {
        0 => return Ok(None),
        1 => reader.read_exact(&mut prefix[1..])?,
        _ => unreachable!(),
    }
    let length = u32::from_le_bytes(prefix) as usize;
    if length == 0 || length > maximum {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native message length is invalid",
        ));
    }
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    Ok(Some(payload))
}

pub(crate) fn write_frame(
    writer: &mut impl Write,
    payload: &[u8],
    maximum: usize,
) -> io::Result<()> {
    if payload.is_empty() || payload.len() > maximum || payload.len() > u32::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "native message length is invalid",
        ));
    }
    writer.write_all(&(payload.len() as u32).to_le_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{read_frame, write_frame};

    #[test]
    fn native_frame_round_trip_uses_little_endian_length_prefix() {
        let mut bytes = Vec::new();
        write_frame(&mut bytes, br#"{"command":"hello"}"#, 64).unwrap();
        assert_eq!(&bytes[..4], &(19u32).to_le_bytes());
        assert_eq!(
            read_frame(&mut Cursor::new(bytes), 64).unwrap().unwrap(),
            br#"{"command":"hello"}"#
        );
    }

    #[test]
    fn native_frame_rejects_oversize_and_truncation_without_allocating_payload_limit() {
        let mut oversized = Cursor::new((65u32).to_le_bytes().to_vec());
        assert_eq!(
            read_frame(&mut oversized, 64).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );

        let mut truncated = Cursor::new([3u32.to_le_bytes().as_slice(), b"ab"].concat());
        assert_eq!(
            read_frame(&mut truncated, 64).unwrap_err().kind(),
            std::io::ErrorKind::UnexpectedEof
        );
    }
}
