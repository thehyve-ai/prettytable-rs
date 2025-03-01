//! Internal only utilities
use std::fmt;
use std::io::{Error, ErrorKind, Write};
use std::str;

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::format::Alignment;

#[cfg(any(not(windows), not(feature = "win_crlf")))]
pub static NEWLINE: &[u8] = b"\n";
#[cfg(all(windows, feature = "win_crlf"))]
pub static NEWLINE: &[u8] = b"\r\n";

/// Internal utility for writing data into a string
pub struct StringWriter {
    string: String,
}

impl StringWriter {
    /// Create a new `StringWriter`
    pub fn new() -> StringWriter {
        StringWriter {
            string: String::new(),
        }
    }

    /// Return a reference to the internally written `String`
    pub fn as_string(&self) -> &str {
        &self.string
    }
}

impl Write for StringWriter {
    fn write(&mut self, data: &[u8]) -> Result<usize, Error> {
        let string = match str::from_utf8(data) {
            Ok(s) => s,
            Err(e) => {
                return Err(Error::new(
                    ErrorKind::Other,
                    format!("Cannot decode utf8 string : {}", e),
                ))
            }
        };
        self.string.push_str(string);
        Ok(data.len())
    }

    fn flush(&mut self) -> Result<(), Error> {
        // Nothing to do here
        Ok(())
    }
}

/// Align/fill a string and print it to `out`
/// If `skip_right_fill` is set to `true`, then no space will be added after the string
/// to complete alignment
pub fn print_align<T: Write + ?Sized>(
    out: &mut T,
    align: Alignment,
    text: &str,
    fill: char,
    size: usize,
    skip_right_fill: bool,
) -> Result<(), Error> {
    let text_len = display_width(text);
    let mut nfill = if text_len < size { size - text_len } else { 0 };
    let n = match align {
        Alignment::LEFT => 0,
        Alignment::RIGHT => nfill,
        Alignment::CENTER => nfill / 2,
    };
    if n > 0 {
        out.write_all(&vec![fill as u8; n])?;
        nfill -= n;
    }
    out.write_all(text.as_bytes())?;
    if nfill > 0 && !skip_right_fill {
        out.write_all(&vec![fill as u8; nfill])?;
    }
    Ok(())
}

/// Return the display width of a unicode string.
/// This functions takes ANSI-escaped color codes into account.
pub fn display_width(text: &str) -> usize {
    #[derive(PartialEq, Eq, Clone, Copy)]
    enum State {
        /// We are not inside any terminal escape.
        Normal,
        /// We have just seen a \u{1b}
        EscapeChar,
        /// We have just seen a [
        OpenBracket,
        /// We just ended the escape by seeing an m
        AfterEscape,
        /// We are inside an OSC sequence: ESC ] ...
        Osc,
        /// We saw ESC inside an OSC sequence, need to check if it's followed by '\'.
        OscEscapeChar,
    }

    let width = UnicodeWidthStr::width(text);
    let mut state = State::Normal;
    let mut hidden = 0;

    for c in text.chars() {
        let normalized = c.to_string();
        match state {
            State::Normal => {
                if c == '\u{1b}' {
                    state = State::EscapeChar;
                }
            }
            State::EscapeChar => {
                if c == '[' {
                    // CSI sequence
                    state = State::OpenBracket;
                } else if c == ']' {
                    // OSC sequence
                    state = State::Osc;
                    hidden += 2;
                } else {
                    // Not recognized, return to normal
                    state = State::Normal;
                }
            }
            State::OpenBracket => {
                // If we still see printable characters here,
                // count them as hidden (ANSI code).
                if c == 'm' {
                    // End of a typical CSI sequence
                    state = State::AfterEscape;
                } else if c == '\u{1b}' {
                    // Another escape inside
                    state = State::EscapeChar;
                }
                if UnicodeWidthChar::width(c).unwrap_or(0) > 0 {
                    hidden += 1;
                }
            }
            State::AfterEscape => {
                // Transition back to normal
                state = State::Normal;
                // The character that ended the escape is hidden as well
                if UnicodeWidthChar::width(c).unwrap_or(0) > 0 {
                    hidden += 1;
                }
            }
            State::Osc => {
                // Inside an OSC sequence, skip everything until we see ESC \
                if c == '\u{1b}' {
                    state = State::OscEscapeChar;
                }
                // If it's printable, hide it (it's part of the OSC sequence).
                if UnicodeWidthChar::width(c).unwrap_or(0) > 0 {
                    hidden += 1;
                }
            }
            State::OscEscapeChar => {
                // If we see '\', it ends the OSC sequence, otherwise stay in OSC
                if c == '\\' {
                    state = State::Normal;
                    hidden += 2;
                } else {
                    state = State::Osc;
                    if UnicodeWidthChar::width(c).unwrap_or(0) > 0 {
                        hidden += 1;
                    }
                }
            }
        }
    }

    assert!(
        width >= hidden,
        "internal error: width {} less than hidden {} on string {:?}",
        width,
        hidden,
        text
    );

    width - hidden
}

/// Wrapper struct which will emit the HTML-escaped version of the contained
/// string when passed to a format string.
pub struct HtmlEscape<'a>(pub &'a str);

impl<'a> fmt::Display for HtmlEscape<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        // Because the internet is always right, turns out there's not that many
        // characters to escape: http://stackoverflow.com/questions/7381974
        let HtmlEscape(s) = *self;
        let pile_o_bits = s;
        let mut last = 0;
        for (i, ch) in s.bytes().enumerate() {
            match ch as char {
                '<' | '>' | '&' | '\'' | '"' => {
                    fmt.write_str(&pile_o_bits[last..i])?;
                    let s = match ch as char {
                        '>' => "&gt;",
                        '<' => "&lt;",
                        '&' => "&amp;",
                        '\'' => "&#39;",
                        '"' => "&quot;",
                        _ => unreachable!(),
                    };
                    fmt.write_str(s)?;
                    last = i + 1;
                }
                _ => {}
            }
        }

        if last < s.len() {
            fmt.write_str(&pile_o_bits[last..])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::Alignment;
    use std::io::Write;

    #[test]
    fn string_writer() {
        let mut out = StringWriter::new();
        out.write_all(b"foo").unwrap();
        out.write_all(b" ").unwrap();
        out.write_all(b"").unwrap();
        out.write_all(b"bar").unwrap();
        assert_eq!(out.as_string(), "foo bar");
    }

    #[test]
    fn display_width_hyperlinks() {
        // Test basic hyperlink
        let just_text = "link text";
        let link = "\x1B]8;;https://example.com\x1B\\link text\x1B]8;;\x1B\\";
        assert_eq!(display_width(link), display_width(just_text)); // "link text" length

        // Test hyperlink with ANSI color
        // let just_text = "colored text";
        // let colored_link =
        //     "\x1B[31m\x1B]8;;https://example.com\x1B\\colored link\x1B]8;;\x1B\\\x1B[0m";
        // assert_eq!(display_width(colored_link), display_width(just_text)); // "colored link" length

        // // Test multiple hyperlinks in one string
        // let multiple_links = "normal \x1B]8;;https://example.com\x1B\\link1\x1B]8;;\x1B\\ and \x1B]8;;https://test.com\x1B\\link2\x1B]8;;\x1B\\";
        // assert_eq!(
        //     display_width(multiple_links),
        //     display_width("normal link1 and link2")
        // );

        // // Test nested formatting
        // let nested = "\x1B]8;;https://example.com\x1B\\\x1B[1mBold Link\x1B[0m\x1B]8;;\x1B\\";
        // assert_eq!(display_width(nested), display_width("Bold Link"));
    }

    #[test]
    fn fill_align() {
        let mut out = StringWriter::new();
        print_align(&mut out, Alignment::RIGHT, "foo", '*', 10, false).unwrap();
        assert_eq!(out.as_string(), "*******foo");

        let mut out = StringWriter::new();
        print_align(&mut out, Alignment::LEFT, "foo", '*', 10, false).unwrap();
        assert_eq!(out.as_string(), "foo*******");

        let mut out = StringWriter::new();
        print_align(&mut out, Alignment::CENTER, "foo", '*', 10, false).unwrap();
        assert_eq!(out.as_string(), "***foo****");

        let mut out = StringWriter::new();
        print_align(&mut out, Alignment::CENTER, "foo", '*', 1, false).unwrap();
        assert_eq!(out.as_string(), "foo");
    }

    #[test]
    fn skip_right_fill() {
        let mut out = StringWriter::new();
        print_align(&mut out, Alignment::RIGHT, "foo", '*', 10, true).unwrap();
        assert_eq!(out.as_string(), "*******foo");

        let mut out = StringWriter::new();
        print_align(&mut out, Alignment::LEFT, "foo", '*', 10, true).unwrap();
        assert_eq!(out.as_string(), "foo");

        let mut out = StringWriter::new();
        print_align(&mut out, Alignment::CENTER, "foo", '*', 10, true).unwrap();
        assert_eq!(out.as_string(), "***foo");

        let mut out = StringWriter::new();
        print_align(&mut out, Alignment::CENTER, "foo", '*', 1, false).unwrap();
        assert_eq!(out.as_string(), "foo");
    }

    #[test]
    fn utf8_error() {
        let mut out = StringWriter::new();
        let res = out.write_all(&[0, 255]);
        assert!(res.is_err());
    }
}
