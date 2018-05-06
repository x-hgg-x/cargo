//! A small module giving you a simple container that allows easy and cheap
//! replacement of parts of its content, with the ability to prevent changing
//! the same parts multiple times.

use std::rc::Rc;
use failure::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
enum State {
    Initial,
    Replaced(Rc<[u8]>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Span {
    /// Start of this span in parent data
    start: usize,
    /// up to end inculding
    end: usize,
    data: State,
}

/// A container that allows easily replacing chunks of its data
#[derive(Debug, Clone, Default)]
pub struct Data {
    original: Vec<u8>,
    parts: Vec<Span>,
}

impl Data {
    /// Create a new data container from a slice of bytes
    pub fn new(data: &[u8]) -> Self {
        Data {
            original: data.into(),
            parts: vec![
                Span {
                    data: State::Initial,
                    start: 0,
                    end: data.len(),
                },
            ],
        }
    }

    /// Render this data as a vector of bytes
    pub fn to_vec(&self) -> Vec<u8> {
        self.parts.iter().fold(Vec::new(), |mut acc, d| {
            match d.data {
                State::Initial => acc.extend_from_slice(&self.original[d.start..d.end]),
                State::Replaced(ref d) => acc.extend_from_slice(&d),
            };
            acc
        })
    }

    /// Replace a chunk of data with the given slice, erroring when this part
    /// was already changed previously.
    pub fn replace_range(
        &mut self,
        from: usize,
        up_to_and_including: usize,
        data: &[u8],
    ) -> Result<(), Error> {
        ensure!(
            from <= up_to_and_including,
            "Invalid range {}...{}, start is larger than end",
            from,
            up_to_and_including
        );
        ensure!(
            up_to_and_including <= self.original.len(),
            "Invalid range {}...{} given, original data is only {} byte long",
            from,
            up_to_and_including,
            self.original.len()
        );

        // Since we error out when replacing an already replaced chunk of data,
        // we can take some shortcuts here. For example, there can be no
        // overlapping replacements -- we _always_ split a chunk of 'initial'
        // data into three[^empty] parts, and there can't ever be two 'initial'
        // parts touching.
        //
        // [^empty]: Leading and trailing ones might be empty if we replace
        // the whole chunk. As an optimization and without loss of generality we
        // don't add empty parts.
        let new_parts = {
            let index_of_part_to_split = self.parts
                .iter()
                .position(|p| p.start <= from && p.end >= up_to_and_including)
                .ok_or_else(|| {
                    use log::Level::Debug;
                    if log_enabled!(Debug) {
                        let slices = self.parts
                            .iter()
                            .map(|p| (p.start, p.end, match p.data {
                                State::Initial => "initial",
                                State::Replaced(..) => "replaced",
                            }))
                            .collect::<Vec<_>>();
                        debug!("no single slice covering {}...{}, current slices: {:?}",
                            from, up_to_and_including, slices,
                        );
                    }

                    format_err!(
                        "Could not replace range {}...{} in file \
                        -- maybe parts of it were already replaced?",
                        from,
                        up_to_and_including
                    )
                })?;

            let part_to_split = &self.parts[index_of_part_to_split];
            ensure!(
                part_to_split.data == State::Initial,
                "Cannot replace slice of data that was already replaced"
            );

            let mut new_parts = Vec::with_capacity(self.parts.len() + 2);

            // Previous parts
            if let Some(ps) = self.parts.get(..index_of_part_to_split) {
                new_parts.extend_from_slice(&ps);
            }

            // Keep initial data on left side of part
            if from > part_to_split.start {
                new_parts.push(Span {
                    start: part_to_split.start,
                    end: from,
                    data: State::Initial,
                });
            }

            // New part
            new_parts.push(Span {
                start: from,
                end: up_to_and_including,
                data: State::Replaced(data.into()),
            });

            // Keep initial data on right side of part
            if up_to_and_including < part_to_split.end {
                new_parts.push(Span {
                    start: up_to_and_including + 1,
                    end: part_to_split.end,
                    data: State::Initial,
                });
            }

            // Following parts
            if let Some(ps) = self.parts.get(index_of_part_to_split + 1..) {
                new_parts.extend_from_slice(&ps);
            }

            new_parts
        };

        self.parts = new_parts;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn str(i: &[u8]) -> &str {
        ::std::str::from_utf8(i).unwrap()
    }

    #[test]
    fn replace_some_stuff() {
        let mut d = Data::new(b"foo bar baz");
        d.replace_range(4, 6, b"lol").unwrap();
        assert_eq!("foo lol baz", str(&d.to_vec()));
    }

    #[test]
    fn replace_a_single_char() {
        let mut d = Data::new(b"let y = true;");
        d.replace_range(4, 4, b"mut y").unwrap();
        assert_eq!("let mut y = true;", str(&d.to_vec()));
    }

    #[test]
    fn replace_multiple_lines() {
        let mut d = Data::new(b"lorem\nipsum\ndolor");

        d.replace_range(6, 10, b"lol").unwrap();
        assert_eq!("lorem\nlol\ndolor", str(&d.to_vec()));

        d.replace_range(12, 17, b"lol").unwrap();
        assert_eq!("lorem\nlol\nlol", str(&d.to_vec()));
    }

    #[test]
    #[should_panic(expected = "Cannot replace slice of data that was already replaced")]
    fn replace_overlapping_stuff_errs() {
        let mut d = Data::new(b"foo bar baz");

        d.replace_range(4, 6, b"lol").unwrap();
        assert_eq!("foo lol baz", str(&d.to_vec()));

        d.replace_range(4, 6, b"lol").unwrap();
    }

    #[test]
    #[should_panic(expected = "original data is only 3 byte long")]
    fn broken_replacements() {
        let mut d = Data::new(b"foo");
        d.replace_range(4, 7, b"lol").unwrap();
    }

    proptest! {
        #[test]
        #[ignore]
        fn new_to_vec_roundtrip(ref s in "\\PC*") {
            assert_eq!(s.as_bytes(), Data::new(s.as_bytes()).to_vec().as_slice());
        }

        #[test]
        #[ignore]
        fn replace_random_chunks(
            ref data in "\\PC*",
            ref replacements in prop::collection::vec(
                (any::<::std::ops::Range<usize>>(), any::<Vec<u8>>()),
                1..1337,
            )
        ) {
            let mut d = Data::new(data.as_bytes());
            for &(ref range, ref bytes) in replacements {
                let _ = d.replace_range(range.start, range.end, bytes);
            }
        }
    }
}
