//! Pairing a second device, without copying a token by hand.
//!
//! The token `omt web` prints is 32 characters of base62. Reading that off a
//! laptop and typing it into a phone is the step people give up at, and it is
//! also the step where a token gets pasted into a chat window "just to get it
//! onto the phone" — at which point it is in somebody else's scrollback.
//!
//! So the terminal prints the invite as a QR. The camera reads it, the phone
//! opens the link, and the token never exists anywhere a person could copy it
//! from.

use qrcode::{EcLevel, QrCode, types::Color};

/// The URL that carries an invite.
///
/// The token goes in the **fragment**. A fragment is never sent to a server:
/// not in the request line, not in an access log, not in the `Referer` of
/// anything the page later loads. The same token in a query string would be
/// written to disk by every proxy between the phone and the instance.
#[must_use]
pub fn invite_url(scheme: &str, bind: &str, token: &str) -> String {
    format!("{scheme}://{bind}/#token={token}")
}

/// The address to put in an invite, given the address the server bound to.
///
/// `0.0.0.0` means "every interface" to a listening socket and nothing at all
/// to a browser — a QR containing it scans perfectly and opens a page that
/// cannot load. The same is true of `::`. So a wildcard bind is rewritten to
/// an address this machine can actually be reached at.
///
/// Everything else is left exactly as given, including a hostname: somebody
/// who passed a name meant that name, and resolving it here would replace a
/// stable address with whatever it happens to point at right now.
#[must_use]
pub fn reachable_address(bind: &str, local_ip: impl FnOnce() -> Option<String>) -> String {
    let Some((host, port)) = bind.rsplit_once(':') else {
        return bind.to_owned();
    };
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if bare != "0.0.0.0" && bare != "::" && !bare.is_empty() {
        return bind.to_owned();
    }
    match local_ip() {
        Some(ip) => format!("{ip}:{port}"),
        // Nothing better to offer. Left as-is rather than guessed at, because a
        // wrong address in a QR is harder to diagnose than an obviously
        // unusable one.
        None => bind.to_owned(),
    }
}

/// This machine's address on the network it can reach the world through.
///
/// Found by opening a UDP socket towards a public address and asking the
/// kernel which local address it chose. No packet is sent — a UDP connect only
/// fixes the route — so this works with no network access and asks the routing
/// table the question directly rather than guessing from an interface list.
#[must_use]
pub fn local_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.0.2.1:80").ok()?;
    Some(socket.local_addr().ok()?.ip().to_string())
}

/// The invite as a block of text a terminal can print.
///
/// Two rows of the code per line of text, using the half-block character, so
/// the result is square on a terminal whose cells are twice as tall as they
/// are wide. Drawn a row per line instead, it comes out stretched to twice its
/// width — which still scans, but only after somebody shrinks their font.
///
/// The quiet zone is four modules wide, as the specification requires. Without
/// it a code printed against a dark background will not scan, and the failure
/// looks like a broken camera rather than a missing margin.
///
/// # Errors
/// Fails if the text is too long to encode, which for a URL of any sane length
/// it is not.
pub fn qr_block(text: &str) -> Result<String, qrcode::types::QrError> {
    // Low correction: this is read off a screen a hand's width away, not from
    // a label on a crate. Higher correction only makes the code denser, and a
    // denser code is a code that needs a bigger terminal window.
    let code = QrCode::with_error_correction_level(text, EcLevel::L)?;
    let width = code.width();
    let modules = code.to_colors();

    const QUIET: usize = 4;
    let dark = |x: isize, y: isize| -> bool {
        if x < 0 || y < 0 || x >= width as isize || y >= width as isize {
            return false;
        }
        modules[y as usize * width + x as usize] == Color::Dark
    };

    let side = width + QUIET * 2;
    let mut out = String::new();
    // Two module-rows at a time: the upper half-block and the lower.
    for row in (0..side).step_by(2) {
        for column in 0..side {
            let x = column as isize - QUIET as isize;
            let top = dark(x, row as isize - QUIET as isize);
            let bottom = dark(x, row as isize + 1 - QUIET as isize);
            // Dark modules are drawn as the *background* colour of a light
            // terminal and vice versa, so this uses block characters on the
            // terminal's own colours rather than forcing black on white: a
            // code that inverts with the user's theme still scans, because
            // scanners handle inversion, but one that assumes a background it
            // does not have does not.
            out.push(match (top, bottom) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            });
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::panic,
    reason = "in a test, expect() is the assertion"
)]
mod tests {
    use super::*;

    #[test]
    fn the_token_travels_in_the_fragment_and_not_the_query() {
        let url = invite_url("http", "127.0.0.1:7777", "omt_c_secret");
        assert!(url.contains("/#token=omt_c_secret"), "{url}");
        assert!(
            !url.contains("?token="),
            "a token in the query string reaches every access log between here and the phone"
        );
    }

    #[test]
    fn a_wildcard_bind_becomes_an_address_a_phone_can_open() {
        // `0.0.0.0` in a QR is the worst kind of broken: it scans first time
        // and opens a page that cannot load.
        assert_eq!(
            reachable_address("0.0.0.0:7777", || Some("192.168.1.20".to_owned())),
            "192.168.1.20:7777"
        );
        assert_eq!(
            reachable_address("[::]:7777", || Some("192.168.1.20".to_owned())),
            "192.168.1.20:7777"
        );
    }

    #[test]
    fn an_address_somebody_chose_is_left_alone() {
        // Including loopback. Somebody who asked for 127.0.0.1 gets 127.0.0.1;
        // the warning about it is printed separately rather than silently
        // rewriting what they typed.
        for bind in ["127.0.0.1:7777", "192.168.1.9:80", "omt.local:7777"] {
            assert_eq!(
                reachable_address(bind, || Some("10.0.0.1".to_owned())),
                bind
            );
        }
    }

    #[test]
    fn a_wildcard_with_no_address_to_offer_is_left_as_it_is() {
        // Rather than guessed at: a wrong address in a QR is harder to work out
        // than an obviously unusable one.
        assert_eq!(reachable_address("0.0.0.0:7777", || None), "0.0.0.0:7777");
    }

    #[test]
    fn a_printed_code_is_square() {
        // Half-blocks are what make it square: a row per line is twice as wide
        // as it is tall, and a stretched code needs the reader to shrink their
        // font before a camera will look at it.
        let block = qr_block("https://example.com/#token=abc").expect("encodes");
        let lines: Vec<&str> = block.lines().collect();
        let width = lines[0].chars().count();
        let height = lines.len();
        assert!(
            width.abs_diff(height * 2) <= 1,
            "{width} wide by {height} lines is not square once each line is two module rows",
        );
    }

    #[test]
    fn every_line_is_the_same_width() {
        // A ragged block is a code with a module missing from the end of a row,
        // and it will not scan.
        let block = qr_block("https://example.com/#token=abc").expect("encodes");
        let widths: Vec<usize> = block.lines().map(|l| l.chars().count()).collect();
        assert!(
            widths.windows(2).all(|w| w[0] == w[1]),
            "ragged: {widths:?}"
        );
    }

    #[test]
    fn the_quiet_zone_is_present_on_every_side() {
        // Four clear modules all round, or a scanner cannot find the code's
        // edge. This is the commonest reason a printed QR does not work.
        let block = qr_block("https://example.com/#token=abc").expect("encodes");
        let lines: Vec<&str> = block.lines().collect();

        // Two blank text lines top and bottom = four module rows.
        assert!(
            lines[0].trim().is_empty() && lines[1].trim().is_empty(),
            "no quiet zone above"
        );
        let last = lines.len() - 1;
        assert!(
            lines[last].trim().is_empty() && lines[last - 1].trim().is_empty(),
            "no quiet zone below"
        );

        for line in &lines {
            let chars: Vec<char> = line.chars().collect();
            assert!(
                chars[..4].iter().all(|c| *c == ' '),
                "no quiet zone to the left"
            );
            assert!(
                chars[chars.len() - 4..].iter().all(|c| *c == ' '),
                "no quiet zone to the right"
            );
        }
    }

    #[test]
    fn the_printed_code_decodes_back_to_the_url() {
        // Everything else here checks the code's *shape*, and a matrix printed
        // transposed or mirrored is the right shape and the wrong code. This is
        // the only test that would catch that: it reads the block back the way
        // a phone camera does.
        let url = invite_url(
            "https",
            "192.168.1.20:7777",
            "omt_c_B2IX4JYoQtzA3AfqZDYPmzxRHMcMRH8W",
        );
        let block = qr_block(&url).expect("encodes");

        // Back into a bitmap, one pixel per module. Each text line is two
        // module rows, which is the mapping under test.
        let lines: Vec<Vec<char>> = block.lines().map(|l| l.chars().collect()).collect();
        let width = lines[0].len();
        let height = lines.len() * 2;
        let mut image = image::GrayImage::new(width as u32, height as u32);
        for (row, line) in lines.iter().enumerate() {
            for (column, cell) in line.iter().enumerate() {
                let (top, bottom) = match cell {
                    '█' => (true, true),
                    '▀' => (true, false),
                    '▄' => (false, true),
                    _ => (false, false),
                };
                let shade = |dark: bool| image::Luma([if dark { 0u8 } else { 255u8 }]);
                image.put_pixel(column as u32, (row * 2) as u32, shade(top));
                image.put_pixel(column as u32, (row * 2 + 1) as u32, shade(bottom));
            }
        }

        let mut prepared = rqrr::PreparedImage::prepare(image);
        let grids = prepared.detect_grids();
        assert_eq!(grids.len(), 1, "a camera would not find exactly one code");
        let (_, decoded) = grids[0].decode().expect("decodes");
        assert_eq!(
            decoded, url,
            "the code scans to something other than the invite"
        );
    }

    #[test]
    fn a_real_token_length_url_still_encodes() {
        // The tokens omt actually mints are 32 base62 characters after a
        // prefix. A code that only encodes the short URLs in these tests would
        // fail on the first real one.
        let url = invite_url(
            "https",
            "192.168.1.20:7777",
            "omt_c_B2IX4JYoQtzA3AfqZDYPmzxRHMcMRH8W",
        );
        assert!(qr_block(&url).is_ok());
    }
}
