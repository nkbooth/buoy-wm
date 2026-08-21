// SPDX-FileCopyrightText: © 2026 Nick Booth
// SPDX-License-Identifier: RPL-1.5
//
// Unless explicitly acquired and licensed from Licensor under another
// license, the contents of this file are subject to the Reciprocal Public
// License ("RPL") Version 1.5, or subsequent versions as allowed by the
// RPL, and You may not copy or use this file in either source code or
// executable form, except in compliance with the terms and conditions of
// the RPL.
//
// All software distributed under the RPL is provided strictly on an "AS
// IS" basis, WITHOUT WARRANTY OF ANY KIND, EITHER EXPRESS OR IMPLIED, AND
// LICENSOR HEREBY DISCLAIMS ALL SUCH WARRANTIES, INCLUDING WITHOUT
// LIMITATION, ANY WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR
// PURPOSE, QUIET ENJOYMENT, OR NON-INFRINGEMENT. See the RPL for specific
// language governing rights and limitations under the RPL.

//! The newline-delimited-JSON framing bound both ends of the IPC socket
//! enforce.
//!
//! The server bounded what it read from clients from the start; neither
//! client bounded what it read back (audit finding E-04), which is only
//! defensible while the server is trustworthy — and a squatted socket path
//! is exactly the case where it is not. One implementation here means a
//! reader cannot be written that forgets the cap.

use std::io::{BufRead, Read};

/// The maximum accepted length of one framed line, in bytes. A line at or
/// beyond this length with no terminating newline is malformed input: the
/// reader stops rather than growing a buffer forever for a hostile or
/// broken peer.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// The three outcomes of a bounded line read. Distinguishing them is the
/// point: a peer that closed the connection and a peer sending an
/// unbounded line need different responses, and collapsing them is what
/// let the clients read without a cap at all.
#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    /// One complete line, with the trailing newline already stripped.
    Complete(Vec<u8>),
    /// The peer closed the connection, either cleanly or mid-line. A
    /// partial line within the cap is treated as EOF because there is
    /// nothing more the peer will send to complete it.
    Eof,
    /// [`MAX_LINE_BYTES`] bytes arrived with no newline in sight.
    Oversize,
}

/// Reads one newline-delimited line from `reader`, refusing to buffer more
/// than [`MAX_LINE_BYTES`].
///
/// I/O errors — including a read deadline expiring — propagate to the
/// caller, which is what lets the server distinguish an idle connection
/// from a broken one.
pub fn read_line_bounded<R: BufRead>(reader: &mut R) -> std::io::Result<Line> {
    let mut buf = Vec::new();
    // `take` bounds the buffer instead of trusting the peer to send a
    // newline: one byte over the cap is enough to tell an oversized line
    // from a line that exactly fills it.
    let read = reader
        .by_ref()
        .take(MAX_LINE_BYTES as u64 + 1)
        .read_until(b'\n', &mut buf)?;
    if read == 0 {
        return Ok(Line::Eof);
    }
    if buf.last() != Some(&b'\n') {
        return if buf.len() > MAX_LINE_BYTES {
            Ok(Line::Oversize)
        } else {
            Ok(Line::Eof)
        };
    }
    buf.pop();
    Ok(Line::Complete(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(input: &[u8]) -> Line {
        let mut reader = std::io::BufReader::new(input);
        read_line_bounded(&mut reader).expect("reading from a slice cannot fail")
    }

    #[test]
    fn a_terminated_line_comes_back_without_its_newline() {
        assert_eq!(read(b"hello\n"), Line::Complete(b"hello".to_vec()));
    }

    #[test]
    fn an_empty_input_is_eof() {
        assert_eq!(read(b""), Line::Eof);
    }

    #[test]
    fn a_partial_line_within_the_cap_is_eof_rather_than_a_line() {
        assert_eq!(read(b"hello"), Line::Eof);
    }

    #[test]
    fn successive_calls_return_successive_lines() {
        let input: &[u8] = b"one\ntwo\n";
        let mut reader = std::io::BufReader::new(input);
        assert_eq!(
            read_line_bounded(&mut reader).unwrap(),
            Line::Complete(b"one".to_vec())
        );
        assert_eq!(
            read_line_bounded(&mut reader).unwrap(),
            Line::Complete(b"two".to_vec())
        );
        assert_eq!(read_line_bounded(&mut reader).unwrap(), Line::Eof);
    }

    #[test]
    fn a_line_exactly_at_the_cap_is_still_accepted() {
        let mut input = vec![b'x'; MAX_LINE_BYTES - 1];
        input.push(b'\n');
        assert_eq!(read(&input), Line::Complete(vec![b'x'; MAX_LINE_BYTES - 1]));
    }

    #[test]
    fn an_unterminated_line_past_the_cap_is_oversize_rather_than_buffered() {
        let input = vec![b'x'; MAX_LINE_BYTES * 2];
        assert_eq!(read(&input), Line::Oversize);
    }
}
