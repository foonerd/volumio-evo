//! Every stderr write from the `tracing-subscriber` fmt layer is prefixed so **journald always**
//! shows [`crate::log_tags::EVO_LINE`] at the start of each log line (works even if `FormatEvent`
//! delegation behaved oddly on some targets).

use std::io::{self, Write};
use tracing_subscriber::fmt::writer::MakeWriter;

/// Use with [`tracing_subscriber::fmt::layer`]. Prefixes the **first** write of each new writer
/// (one writer per event), so multi-chunk lines still get a single leading prefix.
#[derive(Clone, Default, Debug)]
pub struct EvoPrefixedStderr;

impl<'a> MakeWriter<'a> for EvoPrefixedStderr {
    type Writer = PrefixedOnceStderr;

    fn make_writer(&'a self) -> Self::Writer {
        PrefixedOnceStderr {
            inner: io::stderr(),
            need_prefix: true,
        }
    }
}

pub struct PrefixedOnceStderr {
    inner: io::Stderr,
    need_prefix: bool,
}

impl Write for PrefixedOnceStderr {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.need_prefix {
            self.inner
                .write_all(crate::log_tags::EVO_LINE.as_bytes())?;
            self.need_prefix = false;
        }
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
