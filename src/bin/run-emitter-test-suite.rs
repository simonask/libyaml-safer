#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::items_after_statements,
    clippy::let_underscore_untyped,
    clippy::missing_errors_doc,
    clippy::missing_safety_doc,
    clippy::ptr_as_ptr,
    clippy::single_match_else,
    clippy::too_many_lines,
    clippy::unreadable_literal,
    clippy::manual_strip
)]

use libyaml_safer::{Emitter, Encoding, Event, MappingStyle, ScalarStyle, SequenceStyle};
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;

pub(crate) fn test_main(
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    let mut emitter = Emitter::new();

    emitter.set_output(stdout);
    emitter.set_canonical(false);
    emitter.set_unicode(false);

    let mut buf = std::io::BufReader::new(stdin);
    let mut line_buffer = String::with_capacity(1024);
    let mut value_buffer = String::with_capacity(128);

    loop {
        line_buffer.clear();
        let n = buf.read_line(&mut line_buffer)?;
        if n == 0 {
            return Ok(());
        }
        let line = line_buffer.strip_suffix('\n').unwrap_or(&line_buffer);

        let event = if line.starts_with("+STR") {
            Event::stream_start(Encoding::Utf8)
        } else if line.starts_with("-STR") {
            Event::stream_end()
        } else if line.starts_with("+DOC") {
            let implicit = !line[4..].starts_with(" ---");
            Event::document_start(None, &[], implicit)
        } else if line.starts_with("-DOC") {
            let implicit = !line[4..].starts_with(" ...");
            Event::document_end(implicit)
        } else if line.starts_with("+MAP") {
            Event::mapping_start(
                get_anchor('&', line),
                get_tag(line),
                false,
                MappingStyle::Block,
            )
        } else if line.starts_with("-MAP") {
            Event::mapping_end()
        } else if line.starts_with("+SEQ") {
            Event::sequence_start(
                get_anchor('&', line),
                get_tag(line),
                false,
                SequenceStyle::Block,
            )
        } else if line.starts_with("-SEQ") {
            Event::sequence_end()
        } else if line.starts_with("=VAL") {
            let mut style = ScalarStyle::Any;
            let value = get_value(line, &mut value_buffer, &mut style);
            let implicit = get_tag(line).is_none();
            Event::scalar(
                get_anchor('&', line),
                get_tag(line),
                value,
                implicit,
                implicit,
                style,
            )
        } else if line.starts_with("=ALI") {
            Event::alias(get_anchor('*', line).expect("no alias name"))
        } else {
            return Err(format!("Unknown event: '{line}'").into());
        };

        if let Err(err) = emitter.emit(event) {
            return Err(err.into());
        }
    }
}

fn get_anchor(sigil: char, line: &str) -> Option<&str> {
    let (_, from_sigil) = line.split_once(sigil)?;
    if let Some((until_space, _tail)) = from_sigil.split_once(' ') {
        Some(until_space)
    } else if !from_sigil.is_empty() {
        Some(from_sigil)
    } else {
        None
    }
}

fn get_tag(line: &str) -> Option<&str> {
    let (_, from_angle_open) = line.split_once('<')?;
    let (until_angle_close, _) = from_angle_open.split_once('>')?;
    Some(until_angle_close)
}

fn get_value<'a>(line: &str, buffer: &'a mut String, style: &mut ScalarStyle) -> &'a str {
    let mut remainder = line;
    let value = loop {
        let (_before, tail) = match remainder.split_once(' ') {
            Some(parts) => parts,
            None => panic!("invalid line: {line}"),
        };

        *style = match tail.chars().next().expect("string should not be empty") {
            ':' => ScalarStyle::Plain,
            '\'' => ScalarStyle::SingleQuoted,
            '"' => ScalarStyle::DoubleQuoted,
            '|' => ScalarStyle::Literal,
            '>' => ScalarStyle::Folded,
            _ => {
                // This was an anchor, move to the next space.
                remainder = tail;
                continue;
            }
        };
        break &tail[1..];
    };

    buffer.clear();
    // Unescape the value
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            buffer.push(match chars.next().expect("unterminated escape sequence") {
                '\\' => '\\',
                '0' => '\0',
                'b' => '\x08',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                otherwise => panic!("invalid escape character: {otherwise:?}"),
            });
        } else {
            buffer.push(ch);
        }
    }

    &*buffer
}

fn main() -> ExitCode {
    let args = env::args_os().skip(1);
    if args.len() == 0 {
        let _ = writeln!(
            io::stderr(),
            "Usage: run-emitter-test-suite <test.event>...",
        );
        return ExitCode::FAILURE;
    }
    for arg in args {
        let mut stdin = File::open(arg).unwrap();
        let mut stdout = io::stdout();
        let result = test_main(&mut stdin, &mut stdout);
        if let Err(err) = result {
            let _ = writeln!(io::stderr(), "{err}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
