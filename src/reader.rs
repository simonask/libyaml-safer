use std::io::BufRead;

use alloc::collections::VecDeque;

use crate::{Encoding, Error, Result, scanner::Scanner};

const BOM_UTF8: [u8; 3] = [0xef, 0xbb, 0xbf];
const BOM_UTF16LE: [u8; 2] = [0xff, 0xfe];
const BOM_UTF16BE: [u8; 2] = [0xfe, 0xff];

fn yaml_parser_determine_encoding(reader: &mut dyn BufRead) -> Result<Option<Encoding>> {
    let initial_bytes = reader.fill_buf()?;
    if initial_bytes.is_empty() {
        return Ok(None);
    }

    match initial_bytes[0] {
        0xef => {
            let mut bom = [0; 3];
            reader.read_exact(&mut bom)?;
            if bom == BOM_UTF8 {
                Ok(Some(Encoding::Utf8))
            } else {
                Err(Error::reader(
                    "invalid byte order marker",
                    0,
                    i32::from_be_bytes([bom[0], bom[1], bom[2], 0]),
                ))
            }
        }
        0xff | 0xfe => {
            let mut bom = [0; 2];
            reader.read_exact(&mut bom)?;
            if bom == BOM_UTF16LE {
                Ok(Some(Encoding::Utf16Le))
            } else if bom == BOM_UTF16BE {
                Ok(Some(Encoding::Utf16Be))
            } else {
                Err(Error::reader(
                    "invalid byte order marker",
                    0,
                    i32::from_le_bytes([bom[0], bom[1], 0, 0]),
                ))
            }
        }
        _ => Ok(Some(Encoding::Utf8)),
    }
}

// Allowing unsafe code because it is the only efficient way to partially decode
// a string slice from a stream of UTF-8 bytes.
#[allow(unsafe_code)]
fn read_utf8_buffered(
    reader: &mut dyn BufRead,
    out: &mut VecDeque<char>,
    offset: &mut usize,
) -> Result<bool> {
    let available = loop {
        match reader.fill_buf() {
            Ok([]) => return Ok(false),
            Ok(available) => break available,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => (),
            Err(err) => return Err(err.into()),
        }
    };

    match core::str::from_utf8(available) {
        Ok(valid) => {
            let used = valid.len();
            // The entire contents of the input buffer was valid UTF-8.
            for ch in valid.chars() {
                push_char(out, ch, *offset)?;
                *offset += ch.len_utf8();
            }
            reader.consume(used);
            Ok(true)
        }
        Err(err) => {
            let valid_bytes = err.valid_up_to();

            // If some of the buffer contents were valid, append that to the
            // output.
            let valid = unsafe {
                // SAFETY: This is safe because of `valid_up_to()`.
                core::str::from_utf8_unchecked(&available[..valid_bytes])
            };
            for ch in valid.chars() {
                push_char(out, ch, *offset)?;
                *offset += ch.len_utf8();
            }

            match err.error_len() {
                Some(_invalid_len) => Err(Error::reader(
                    "invalid UTF-8",
                    *offset,
                    available[valid_bytes] as _,
                )),
                None => {
                    if valid_bytes != 0 {
                        // Some valid UTF-8 characters were present, and the
                        // tail end of the buffer was an incomplete sequence.
                        // Leave the incomplete sequence in the buffer.
                        reader.consume(valid_bytes);
                        Ok(true)
                    } else {
                        // The beginning of the buffer was an incomplete UTF-8
                        // sequence. Read the whole character unbuffered.
                        //
                        // This will return `UnexpectedEof` if the sequence
                        // cannot be completed. Note that `read_exact()` handles
                        // interrupt automatically.
                        let initial = available[0];
                        read_utf8_char_unbuffered(reader, out, initial, offset)?;
                        Ok(true)
                    }
                }
            }
        }
    }
}

fn read_utf8_char_unbuffered(
    reader: &mut dyn BufRead,
    out: &mut VecDeque<char>,
    initial: u8,
    offset: &mut usize,
) -> Result<()> {
    let width = utf8_char_width(initial);
    let mut buffer = [0; 4];
    reader.read_exact(&mut buffer[..width])?;
    if let Ok(valid) = core::str::from_utf8(&buffer[..width]) {
        // We read a whole, valid character.
        let ch = match valid.chars().next() {
            Some(ch) => ch,
            None => unreachable!(),
        };
        push_char(out, ch, *offset)?;
        *offset += width;
        Ok(())
    } else {
        // Since we read the exact character width, the only
        // possible error here is invalid Unicode.
        Err(Error::reader("invalid UTF-8", *offset, buffer[0] as _))
    }
}

fn read_utf16_buffered<const BIG_ENDIAN: bool>(
    reader: &mut dyn BufRead,
    out: &mut VecDeque<char>,
    offset: &mut usize,
) -> Result<bool> {
    let available = loop {
        match reader.fill_buf() {
            Ok([]) => return Ok(false),
            Ok(available) => break available,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => (),
            Err(err) => return Err(err.into()),
        }
    };

    let chunks = available.chunks_exact(2).map(|chunk| {
        let (a, b) = match chunk {
            [a, b] => (a, b),
            _ => unreachable!(),
        };
        if BIG_ENDIAN {
            u16::from_be_bytes([*a, *b])
        } else {
            u16::from_le_bytes([*a, *b])
        }
    });

    let mut used = 0;
    for ch in core::char::decode_utf16(chunks) {
        match ch {
            Ok(ch) => {
                push_char(out, ch, *offset)?;
                let n = ch.len_utf16();
                *offset += n;
                used += n;
            }
            Err(_) => {
                // An unpaired surrogate may either be a corrupt stream, but it
                // can also be that the buffer just happens to contain the first
                // half of a surrogate pair. Consume all of the valid bytes in
                // the buffer first, and then handle the unpaired surrogate in
                // the "slow" path (`read_utf16_char_unbuffered`) the next time
                // we are called.
                break;
            }
        }
    }

    if used != 0 {
        reader.consume(used);
        *offset += used;
        Ok(true)
    } else {
        debug_assert!(!available.is_empty() && available.len() < 2);
        read_utf16_char_unbuffered::<BIG_ENDIAN>(reader, out, offset)?;
        Ok(true)
    }
}

fn read_utf16_char_unbuffered<const BIG_ENDIAN: bool>(
    reader: &mut dyn BufRead,
    out: &mut VecDeque<char>,
    offset: &mut usize,
) -> Result<()> {
    let mut buffer = [0; 2];
    reader.read_exact(&mut buffer)?;
    let first = if BIG_ENDIAN {
        u16::from_be_bytes(buffer)
    } else {
        u16::from_le_bytes(buffer)
    };

    if is_utf16_surrogate(first) {
        reader.read_exact(&mut buffer)?;
        let second = if BIG_ENDIAN {
            u16::from_be_bytes(buffer)
        } else {
            u16::from_le_bytes(buffer)
        };

        match core::char::decode_utf16([first, second]).next() {
            Some(Ok(ch)) => {
                push_char(out, ch, *offset)?;
                *offset += 4;
                Ok(())
            }
            Some(Err(err)) => Err(Error::reader(
                "invalid UTF-16",
                *offset,
                err.unpaired_surrogate() as _,
            )),
            None => unreachable!(),
        }
    } else {
        match core::char::decode_utf16([first]).next() {
            Some(Ok(ch)) => {
                push_char(out, ch, *offset)?;
                *offset += 2;
                Ok(())
            }
            Some(Err(_)) | None => unreachable!(),
        }
    }
}

fn utf8_char_width(initial: u8) -> usize {
    if initial & 0x80 == 0 {
        1
    } else if initial & 0xE0 == 0xC0 {
        2
    } else if initial & 0xF0 == 0xE0 {
        3
    } else if initial & 0xF8 == 0xF0 {
        4
    } else {
        0
    }
}

fn is_utf16_surrogate(value: u16) -> bool {
    matches!(value, 0xD800..=0xDFFF)
}

fn push_char(out: &mut VecDeque<char>, ch: char, offset: usize) -> Result<()> {
    if !(ch == '\x09'
        || ch == '\x0A'
        || ch == '\x0D'
        || ch >= '\x20' && ch <= '\x7E'
        || ch == '\u{0085}'
        || ch >= '\u{00A0}' && ch <= '\u{D7FF}'
        || ch >= '\u{E000}' && ch <= '\u{FFFD}'
        || ch >= '\u{10000}' && ch <= '\u{10FFFF}')
    {
        return Err(Error::reader(
            "control characters are not allowed",
            offset,
            ch as _,
        ));
    }
    out.push_back(ch);
    Ok(())
}

pub(crate) fn yaml_parser_update_buffer<R: BufRead>(
    parser: &mut Scanner<R>,
    length: usize,
) -> Result<()> {
    let reader = parser.read_handler.as_mut().expect("no read handler");
    if parser.buffer.len() >= length {
        return Ok(());
    }
    if parser.encoding == Encoding::Any {
        if let Some(encoding) = yaml_parser_determine_encoding(reader)? {
            parser.encoding = encoding;
        } else {
            parser.eof = true;
            return Ok(());
        }
    }

    while parser.buffer.len() < length {
        if parser.eof {
            return Ok(());
        }

        let not_eof = match parser.encoding {
            Encoding::Any => unreachable!(),
            Encoding::Utf8 => read_utf8_buffered(reader, &mut parser.buffer, &mut parser.offset)?,
            Encoding::Utf16Le => {
                read_utf16_buffered::<false>(reader, &mut parser.buffer, &mut parser.offset)?
            }
            Encoding::Utf16Be => {
                read_utf16_buffered::<true>(reader, &mut parser.buffer, &mut parser.offset)?
            }
        };
        if !not_eof {
            parser.eof = true;
            return Ok(());
        }
    }

    if parser.offset >= (!0_usize).wrapping_div(2_usize) {
        return Err(Error::reader("input is too long", parser.offset, -1));
    }
    Ok(())
}
