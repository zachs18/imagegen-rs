use std::{borrow::Cow, collections::VecDeque, iter::Peekable};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HasArgument {
    /// This short option does not have an argument. Further characters in the same parameter are parsed as short options.
    /// This long option does not have an argument.
    No,
    /// This short option has an argument. It may be specified in the same parameter "-oarg" or in the next "-o arg" or with an `=` "-o=arg".
    /// This long option does not have an argument.  It may be specified in the next parameter "--out arg" or with an `=` "--out=arg".
    Yes,
    /// This short option has an optional argument. It may be specified in the same parameter "-oarg" or in the next "-o arg" or with an `=` "-o=arg".
    /// (for short) If the next parameter starts with `-`, it will not be considered an argument for this option.
    ///
    /// This long option has an optional argument. It may be specified in the next parameter "--out arg" or with an `=` "--out=arg".
    /// (for long) If the next parameter starts with `-`, it will not be considered an argument for this option.
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Opt {
    pub short: Option<char>,
    pub long: Option<Cow<'static, str>>,
    pub has_argument: HasArgument,
}

impl Opt {
    pub fn short(short: char, arg: HasArgument) -> Self {
        Opt {
            short: Some(short),
            long: None,
            has_argument: arg,
        }
    }

    pub fn long(long: impl Into<Cow<'static, str>>, arg: HasArgument) -> Self {
        Opt {
            short: None,
            long: Some(long.into()),
            has_argument: arg,
        }
    }

    pub fn short_long(short: char, long: impl Into<Cow<'static, str>>, arg: HasArgument) -> Self {
        Opt {
            short: Some(short),
            long: Some(long.into()),
            has_argument: arg,
        }
    }
}

pub struct Getopt {
    options: Vec<Opt>,
}

impl Getopt {
    /// Assumes the program name is NOT in the iterator.
    pub fn parse<'a, I: IntoIterator<Item = &'a str>>(
        &'a self,
        args: I,
    ) -> GetoptIter<'a, I::IntoIter> {
        GetoptIter {
            opts: &self.options,
            args: args.into_iter().peekable(),
            backlog: VecDeque::new(),
            found_dash_dash: false,
        }
    }

    pub fn add_option(&mut self, opt: Opt) -> Result<(), (&'static str, Opt)> {
        if opt.short.is_none() && opt.long.is_none() {
            return Err(("short and long cannot both be None", opt));
        } else if opt.short.is_some() && "\0-=".contains(opt.short.unwrap()) {
            return Err(("short option cannot be '\0', '-', or '='", opt));
        } else if opt.long.is_some() && opt.long.as_ref().unwrap().len() == 0 {
            return Err(("long option cannot be empty string", opt));
        } else if opt.long.is_some()
            && memchr::memchr3(b'\0', b'-', b'=', opt.long.as_ref().unwrap().as_bytes()).is_some()
        {
            return Err(("long option cannot contain '\0', '-', or '='", opt));
        } else if let Some(existing_opt) = self.options.iter().find(|e_opt| {
            (e_opt.short.is_some() && e_opt.short == opt.short)
                || (e_opt.long.is_some() && e_opt.long == opt.long)
        }) {
            if existing_opt.short == opt.short {
                return Err(("duplicate short option", opt));
            } else {
                return Err(("duplicate long option", opt));
            }
        }
        self.options.push(opt);
        Ok(())
    }

    pub fn from_iter(iter: impl IntoIterator<Item = Opt>) -> Result<Self, (&'static str, Opt)> {
        let iter = iter.into_iter();
        let mut this = Getopt {
            options: Vec::with_capacity(iter.size_hint().0),
        };
        for opt in iter {
            this.add_option(opt)?;
        }
        Ok(this)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GetoptItem<'a> {
    Opt { opt: &'a Opt, arg: Option<&'a str> },
    NonOpt(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GetoptError<'a> {
    // Includes the case where a recognized short opt did not have a required argument or had an unexpected argument (with '=').
    UnrecognizedShortOpt { opt: char, arg: Option<&'a str> },
    // Includes the case where a recognized long opt did not have a required argument or had an unexpected argument (with '=').
    UnrecognizedLongOpt { opt: &'a str, arg: Option<&'a str> },
}

pub struct GetoptIter<'a, I: Iterator<Item = &'a str>> {
    opts: &'a [Opt],
    args: Peekable<I>,
    backlog: VecDeque<Result<GetoptItem<'a>, GetoptError<'a>>>,
    // After "--", return all arguments as NonOpt
    found_dash_dash: bool,
}

impl<'a, I: Iterator<Item = &'a str>> Iterator for GetoptIter<'a, I> {
    type Item = Result<GetoptItem<'a>, GetoptError<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(item) = self.backlog.pop_front() {
            return Some(item);
        } else if self.found_dash_dash {
            return Some(Ok(GetoptItem::NonOpt(self.args.next()?)));
        }
        let opt = self.args.next()?;
        if opt == "--" {
            self.found_dash_dash = true;
            Some(Ok(GetoptItem::NonOpt(self.args.next()?)))
        } else if opt.starts_with("--") {
            let arg = &opt[2..]; // skip '--'
            let (opt, arg) = if let Some(idx) = arg.find('=') {
                (&arg[..idx], Some(&arg[idx + 1..]))
            } else {
                (arg, None)
            };
            let r_opt = match self
                .opts
                .iter()
                .find(|r_opt| Some(opt) == r_opt.long.as_deref())
            {
                Some(r_opt) => r_opt,
                None => return Some(Err(GetoptError::UnrecognizedLongOpt { opt, arg })),
            };
            match (r_opt.has_argument, arg) {
                // Correct, return immediately
                (HasArgument::No, None)
                | (HasArgument::Yes, Some(_))
                | (HasArgument::Optional, Some(_)) => Some(Ok(GetoptItem::Opt { opt: r_opt, arg })),
                // Incorrect, return immediately
                (HasArgument::No, Some(_)) => {
                    Some(Err(GetoptError::UnrecognizedLongOpt { opt, arg }))
                }
                // May require additional parsing
                (HasArgument::Yes, None) => match self.args.next() {
                    Some(arg) => Some(Ok(GetoptItem::Opt {
                        opt: r_opt,
                        arg: Some(arg),
                    })),
                    None => Some(Err(GetoptError::UnrecognizedLongOpt { opt, arg })),
                },
                (HasArgument::Optional, None) => match self.args.peek() {
                    Some(arg) if !arg.starts_with('-') => Some(Ok(GetoptItem::Opt {
                        opt: r_opt,
                        arg: self.args.next(),
                    })),
                    Some(_) | None => Some(Ok(GetoptItem::Opt {
                        opt: r_opt,
                        arg: None,
                    })),
                },
            }
        } else if opt.starts_with("-") {
            // '-' can be used to force an optional-arg opt to not have an arg, e.g.
            // (a, b no arg, c optional arg)
            // -abc - nonopt
            // -> Short('a'), Short('b'), Short('c', None), NonOpt("nonopt")

            // Possibilities:
            // 1. -abcarg=arg
            // 2. -abc=arg=arg
            // 3. -abc arg=arg

            let mut opt = opt[1..].chars(); // skip '-'
            loop {
                // Take one char from it each time, until we reach an arg-having opt, or an unrecognized opt
                let c_opt = match opt.next() {
                    Some(c_opt) => c_opt,
                    None => break,
                };
                let r_opt = match self.opts.iter().find(|r_opt| Some(c_opt) == r_opt.short) {
                    Some(r_opt) => r_opt,
                    None => {
                        // Only assume the unrecognized shortopt has an arg if its explicit with '='
                        if opt.as_str().starts_with('=') {
                            self.backlog
                                .push_back(Err(GetoptError::UnrecognizedShortOpt {
                                    opt: c_opt,
                                    arg: Some(&opt.as_str()[1..]),
                                }));
                            break;
                        } else {
                            self.backlog
                                .push_back(Err(GetoptError::UnrecognizedShortOpt {
                                    opt: c_opt,
                                    arg: None,
                                }));
                            continue;
                        }
                    }
                };

                match (r_opt.has_argument, opt.as_str()) {
                    (HasArgument::No, arg) if arg.starts_with('=') => {
                        self.backlog
                            .push_back(Err(GetoptError::UnrecognizedShortOpt {
                                opt: c_opt,
                                arg: Some(&arg[1..]),
                            }));
                        break;
                    }
                    (HasArgument::No, _) => self.backlog.push_back(Ok(GetoptItem::Opt {
                        opt: r_opt,
                        arg: None,
                    })),
                    (HasArgument::Yes, arg) if arg.len() == 0 => {
                        self.backlog.push_back(match self.args.next() {
                            Some(arg) => Ok(GetoptItem::Opt {
                                opt: r_opt,
                                arg: Some(arg),
                            }),
                            None => Err(GetoptError::UnrecognizedShortOpt {
                                opt: c_opt,
                                arg: None,
                            }),
                        });
                        break;
                    }
                    (HasArgument::Yes, arg) if arg.starts_with('=') => {
                        self.backlog.push_back(Ok(GetoptItem::Opt {
                            opt: r_opt,
                            arg: Some(&arg[1..]),
                        }));
                        break;
                    }
                    (HasArgument::Yes, arg) => {
                        self.backlog.push_back(Ok(GetoptItem::Opt {
                            opt: r_opt,
                            arg: Some(arg),
                        }));
                        break;
                    }
                    (HasArgument::Optional, arg) if arg.len() == 0 => {
                        self.backlog.push_back(match self.args.peek() {
                            Some(arg) if !arg.starts_with('-') => Ok(GetoptItem::Opt {
                                opt: r_opt,
                                arg: self.args.next(),
                            }),
                            Some(_) | None => Ok(GetoptItem::Opt {
                                opt: r_opt,
                                arg: None,
                            }),
                        });
                        break;
                    }
                    (HasArgument::Optional, arg) if arg.starts_with('=') => {
                        self.backlog.push_back(Ok(GetoptItem::Opt {
                            opt: r_opt,
                            arg: Some(&arg[1..]),
                        }));
                        break;
                    }
                    (HasArgument::Optional, arg) => {
                        self.backlog.push_back(Ok(GetoptItem::Opt {
                            opt: r_opt,
                            arg: Some(arg),
                        }));
                        break;
                    }
                }
            }
            // should use backlog, unless this was '-'
            // FIXME: possibility of stack overflow with a lot of consecutive malicious '-' arguments
            // FIXME: maybe put whole function in `'tailcall: loop { ... }`, and use `continue 'tailcall;`
            self.next()
        } else {
            // NonOpt
            Some(Ok(GetoptItem::NonOpt(opt)))
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Getopt, GetoptError, GetoptItem, HasArgument, Opt};

    #[test]
    fn basic_short() {
        let a = Opt::short('a', HasArgument::No);
        let b = Opt::short('b', HasArgument::No);
        let c = Opt::short('c', HasArgument::Optional);
        let getopt = Getopt::from_iter([a.clone(), b.clone(), c.clone()]).unwrap();

        assert_eq!(
            getopt.parse(["-abc"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt { opt: &b, arg: None }),
                Ok(GetoptItem::Opt { opt: &c, arg: None }),
            ]
        );

        assert_eq!(
            getopt.parse(["-abcarg=arg"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt { opt: &b, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &c,
                    arg: Some("arg=arg")
                }),
            ]
        );

        assert_eq!(
            getopt.parse(["-abc=arg=arg"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt { opt: &b, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &c,
                    arg: Some("arg=arg")
                }),
            ]
        );

        assert_eq!(
            getopt.parse(["-abc", "arg=arg"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt { opt: &b, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &c,
                    arg: Some("arg=arg")
                }),
            ]
        );
    }

    #[test]
    fn short_missing_arg() {
        let a = Opt::short('a', HasArgument::No);
        let b = Opt::short('b', HasArgument::Yes);
        let c = Opt::short('c', HasArgument::Optional);
        let getopt = Getopt::from_iter([a.clone(), b.clone(), c.clone()]).unwrap();

        assert_eq!(
            getopt.parse(["-abc"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &b,
                    arg: Some("c")
                }),
            ]
        );

        assert_eq!(
            getopt.parse(["-abcarg=arg"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &b,
                    arg: Some("carg=arg")
                }),
            ]
        );

        assert_eq!(
            getopt.parse(["-abc=arg=arg"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &b,
                    arg: Some("c=arg=arg")
                }),
            ]
        );

        assert_eq!(
            getopt.parse(["-abc", "arg=arg"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &b,
                    arg: Some("c")
                }),
                Ok(GetoptItem::NonOpt("arg=arg")),
            ]
        );

        assert_eq!(
            getopt.parse(["-ab", "arg=arg"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &b,
                    arg: Some("arg=arg")
                }),
            ]
        );

        assert_eq!(
            getopt.parse(["-ab", "-carg=arg"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &b,
                    arg: Some("-carg=arg")
                }),
            ]
        );

        assert_eq!(
            getopt.parse(["-ab"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Err(GetoptError::UnrecognizedShortOpt {
                    opt: 'b',
                    arg: None
                }),
            ]
        );
    }

    #[test]
    fn basic_long() {
        let a = Opt::long("a", HasArgument::No);
        let b = Opt::long("b", HasArgument::No);
        let c = Opt::long("c", HasArgument::Optional);
        let getopt = Getopt::from_iter([a.clone(), b.clone(), c.clone()]).unwrap();

        assert_eq!(
            getopt.parse(["--a", "--b", "--c"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt { opt: &b, arg: None }),
                Ok(GetoptItem::Opt { opt: &c, arg: None }),
            ]
        );

        assert_eq!(
            getopt
                .parse(["--a", "--b", "--c=arg=arg"])
                .collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt { opt: &b, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &c,
                    arg: Some("arg=arg")
                }),
            ]
        );

        assert_eq!(
            getopt
                .parse(["--a", "--b", "--c", "arg=arg"])
                .collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt { opt: &b, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &c,
                    arg: Some("arg=arg")
                }),
            ]
        );

        assert_eq!(
            getopt
                .parse(["--a", "--b", "--carg=arg"])
                .collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt { opt: &b, arg: None }),
                Err(GetoptError::UnrecognizedLongOpt {
                    opt: "carg",
                    arg: Some("arg")
                }),
            ]
        );
    }

    #[test]
    fn long_missing_arg() {
        let a = Opt::long("a", HasArgument::No);
        let b = Opt::long("b", HasArgument::Yes);
        let c = Opt::long("c", HasArgument::Optional);
        let getopt = Getopt::from_iter([a.clone(), b.clone(), c.clone()]).unwrap();

        assert_eq!(
            getopt
                .parse(["--a", "--b", "--c=arg=arg"])
                .collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &b,
                    arg: Some("--c=arg=arg")
                }),
            ]
        );

        assert_eq!(
            getopt
                .parse(["--a", "--b=", "--c=arg=arg"])
                .collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Ok(GetoptItem::Opt {
                    opt: &b,
                    arg: Some("")
                }),
                Ok(GetoptItem::Opt {
                    opt: &c,
                    arg: Some("arg=arg")
                }),
            ]
        );

        assert_eq!(
            getopt.parse(["--a", "--b"]).collect::<Vec<_>>(),
            vec![
                Ok(GetoptItem::Opt { opt: &a, arg: None }),
                Err(GetoptError::UnrecognizedLongOpt {
                    opt: "b",
                    arg: None
                }),
            ]
        );
    }
}
