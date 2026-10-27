//! Prints a token stream on one line with rustfmt-like spacing.
//!
//! The expression printer uses it for the parts it does not walk itself:
//! paths, types, generic arguments, patterns, macro arguments, closure
//! parameters and blocks. The spacing comes from the tokens alone. A `<`
//! after a `CamelCase` word opens generic arguments, so a comparison with a
//! value named that way, such as `Limit < n`, prints as `Limit<n`.

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

/// Prints `tokens` on one line.
pub(crate) fn compact(tokens: TokenStream) -> String {
    let mut printer = Printer {
        output: String::new(),
        previous: Previous::Start,
        generic_depth: 0,
        in_closure_params: false,
    };
    printer.print(&flatten(tokens));
    printer.output
}

enum Token {
    Word(String),
    Literal(String),
    /// Adjacent punctuation characters, such as `::`, `==` or `..=`.
    Operator(String),
    Group(Delimiter, Vec<Token>),
}

/// Merges adjacent punctuation into operators and inlines invisible groups.
fn flatten(tokens: TokenStream) -> Vec<Token> {
    let mut output = Vec::new();
    let mut operator = String::new();
    for tree in tokens {
        if let TokenTree::Punct(punct) = &tree {
            operator.push(punct.as_char());
            if punct.spacing() == Spacing::Alone {
                output.push(Token::Operator(std::mem::take(&mut operator)));
            }
            continue;
        }
        if !operator.is_empty() {
            output.push(Token::Operator(std::mem::take(&mut operator)));
        }
        match tree {
            TokenTree::Ident(ident) => output.push(Token::Word(ident.to_string())),
            // A string literal written across source lines stays on one.
            TokenTree::Literal(literal) => output.push(Token::Literal(
                literal
                    .to_string()
                    .replace("\r\n", "\\n")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r"),
            )),
            TokenTree::Group(group) if group.delimiter() == Delimiter::None => {
                output.extend(flatten(group.stream()));
            }
            TokenTree::Group(group) => {
                output.push(Token::Group(group.delimiter(), flatten(group.stream())));
            }
            TokenTree::Punct(_) => unreachable!("punctuation is merged above"),
        }
    }
    if !operator.is_empty() {
        output.push(Token::Operator(operator));
    }
    output
}

#[derive(Clone, PartialEq)]
enum Previous {
    Start,
    Word,
    Keyword,
    Literal,
    Operator(String),
    /// A token nothing is spaced away from: a prefix operator, a macro
    /// name's `!`, `#`, a lifetime's `'`, or a closure's opening `|`.
    Prefix,
    /// A `<` that opens generic arguments.
    GenericOpen,
    /// A `>` that closes generic arguments.
    GenericClose,
    Open(Delimiter),
    Close(Delimiter),
}

struct Printer {
    output: String,
    previous: Previous,
    generic_depth: usize,
    in_closure_params: bool,
}

impl Printer {
    fn print(&mut self, tokens: &[Token]) {
        for (index, token) in tokens.iter().enumerate() {
            let next = tokens.get(index + 1);
            match token {
                Token::Word(word) => {
                    self.separate_if(self.space_before_word());
                    self.output.push_str(word);
                    self.previous = if is_keyword(word) {
                        Previous::Keyword
                    } else {
                        Previous::Word
                    };
                }
                Token::Literal(literal) => {
                    self.separate_if(self.space_before_word());
                    self.output.push_str(literal);
                    self.previous = Previous::Literal;
                }
                Token::Operator(operator) => self.print_operator(operator, next),
                Token::Group(delimiter, inner) => self.print_group(*delimiter, inner),
            }
        }
    }

    /// Writes a space when `space` asks for one and the previous token
    /// allows one after it.
    fn separate_if(&mut self, space: bool) {
        let allowed = !matches!(
            self.previous,
            Previous::Start | Previous::Open(_) | Previous::Prefix | Previous::GenericOpen
        );
        if space && allowed {
            self.output.push(' ');
        }
    }

    fn space_before_word(&self) -> bool {
        match &self.previous {
            Previous::Operator(operator) => !binds_to_next(operator),
            _ => true,
        }
    }

    fn print_operator(&mut self, operator: &str, next: Option<&Token>) {
        let after_macro_name = operator == "!"
            && self.previous == Previous::Word
            && matches!(next, Some(Token::Group(..)));
        let prefix_position = match &self.previous {
            Previous::Operator(previous) => previous != "?",
            Previous::Start
            | Previous::Open(_)
            | Previous::Prefix
            | Previous::Keyword
            | Previous::GenericOpen => true,
            _ => false,
        };
        let is_prefix = matches!(operator, "&" | "&&" | "*" | "-" | "!")
            && prefix_position
            && !after_macro_name;
        let closure_open =
            matches!(operator, "|" | "||") && !self.in_closure_params && prefix_position;
        let closure_close = operator == "|" && self.in_closure_params;
        let opens_generics = self.opens_generics(operator);
        let closes_generics = self.generic_depth > 0 && matches!(operator, ">" | ">>");

        let space = !(after_macro_name
            || closure_close
            || opens_generics
            || closes_generics
            || matches!(operator, "," | ";" | "." | "?" | "::" | "::<" | ":")
            || (matches!(operator, ".." | "..=" | "...") && self.range_binds_left()));
        self.separate_if(space);
        self.output.push_str(operator);

        self.previous = if opens_generics {
            self.generic_depth += operator.matches('<').count();
            Previous::GenericOpen
        } else if closes_generics {
            self.generic_depth = self
                .generic_depth
                .saturating_sub(operator.matches('>').count());
            Previous::GenericClose
        } else if after_macro_name || is_prefix || matches!(operator, "#" | "'") {
            Previous::Prefix
        } else if closure_open && operator == "|" {
            self.in_closure_params = true;
            Previous::Prefix
        } else {
            if closure_close {
                self.in_closure_params = false;
            }
            Previous::Operator(operator.to_owned())
        };
    }

    fn opens_generics(&self, operator: &str) -> bool {
        match operator {
            "::<" => true,
            "<" | "<<" => {
                self.generic_depth > 0
                    || matches!(self.previous, Previous::Start | Previous::GenericClose)
                    || matches!(self.previous, Previous::Operator(ref previous) if previous == "::")
                    || self.output_ends_with_type_name()
            }
            _ => false,
        }
    }

    /// `Vec<`, `Option<`: a `<` after a `CamelCase` word starts generic
    /// arguments. Locals are lowercase and constants are `SCREAMING_CASE`,
    /// so `a < b` and `LIMIT < n` stay comparisons.
    fn output_ends_with_type_name(&self) -> bool {
        self.previous == Previous::Word
            && self
                .output
                .rsplit(|c: char| !c.is_alphanumeric() && c != '_')
                .next()
                .is_some_and(|word| {
                    word.starts_with(char::is_uppercase) && word.chars().any(char::is_lowercase)
                })
    }

    fn range_binds_left(&self) -> bool {
        matches!(
            self.previous,
            Previous::Word | Previous::Literal | Previous::Close(_) | Previous::GenericClose
        )
    }

    fn print_group(&mut self, delimiter: Delimiter, inner: &[Token]) {
        let space = match delimiter {
            Delimiter::Brace | Delimiter::None => true,
            Delimiter::Parenthesis | Delimiter::Bracket => match &self.previous {
                Previous::Word
                | Previous::Literal
                | Previous::GenericClose
                | Previous::Close(Delimiter::Parenthesis | Delimiter::Bracket) => false,
                Previous::Operator(operator) => operator != "?",
                _ => true,
            },
        };
        self.separate_if(space);
        let (open, close) = match delimiter {
            Delimiter::Parenthesis => ("(", ")"),
            Delimiter::Bracket => ("[", "]"),
            Delimiter::Brace => ("{", "}"),
            Delimiter::None => ("", ""),
        };
        self.output.push_str(open);
        self.previous = Previous::Open(delimiter);
        let padded = delimiter == Delimiter::Brace && !inner.is_empty();
        if padded {
            self.output.push(' ');
        }
        let closure_params = self.in_closure_params;
        self.in_closure_params = false;
        self.print(inner);
        self.in_closure_params = closure_params;
        if padded {
            self.output.push(' ');
        }
        self.output.push_str(close);
        self.previous = Previous::Close(delimiter);
    }
}

/// Operators written without a space after them.
fn binds_to_next(operator: &str) -> bool {
    matches!(operator, "." | "::" | "::<" | ".." | "..=" | "...")
}

fn is_keyword(word: &str) -> bool {
    matches!(
        word,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "static"
            | "struct"
            | "trait"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "yield"
    )
}

#[cfg(test)]
mod tests {
    use super::compact;
    use quote::quote;

    #[test]
    fn method_chain_and_turbofish_have_no_inner_spaces() {
        let printed = compact(quote!(items.iter().map(str::trim).collect::<Vec<_>>()));
        assert_eq!(printed, "items.iter().map(str::trim).collect::<Vec<_>>()");
    }

    #[test]
    fn closure_params_and_body_are_separated_by_one_space() {
        let printed = compact(quote!(|x: &str, (a, b)| x.len() > a - b));
        assert_eq!(printed, "|x: &str, (a, b)| x.len() > a - b");
    }

    #[test]
    fn empty_closure_params_keep_the_body_apart() {
        assert_eq!(compact(quote!(move || *count + 1)), "move || *count + 1");
    }

    #[test]
    fn prefix_operators_bind_to_their_operand() {
        assert_eq!(
            compact(quote!(!(a && b) || -x * &y)),
            "!(a && b) || -x * &y"
        );
        assert_eq!(compact(quote!(a - -1)), "a - -1");
    }

    #[test]
    fn macro_calls_keep_name_bang_and_delimiter_together() {
        assert_eq!(compact(quote!(vec![1, 2, 3])), "vec![1, 2, 3]");
        assert_eq!(compact(quote!(format!("{x}"))), "format!(\"{x}\")");
    }

    #[test]
    fn block_statements_are_spaced_inside_braces() {
        let printed = compact(quote!({
            let x = f(1)?;
            x
        }));
        assert_eq!(printed, "{ let x = f(1)?; x }");
        assert_eq!(compact(quote!({})), "{}");
    }

    #[test]
    fn range_operators_bind_to_their_operands() {
        let printed = compact(quote!(Point { x: a..=b, ..base }));
        assert_eq!(printed, "Point { x: a..=b, ..base }");
        assert_eq!(compact(quote!(items[..2])), "items[..2]");
    }

    #[test]
    fn match_arms_are_spaced_like_rustfmt_and_lifetimes_bind_to_their_name() {
        // Parsed from text: rustfmt would add a trailing comma inside `quote!`.
        let source = "match v { Some(x) if x > 1 => x, _ => 0 }";
        let tokens = source.parse().expect("test source tokenizes");
        assert_eq!(compact(tokens), source);
        assert_eq!(compact(quote!(|s: &'a str| s)), "|s: &'a str| s");
    }

    #[test]
    fn generic_arguments_have_no_inner_spaces_in_paths_and_types() {
        assert_eq!(
            compact(quote!(<T as Default>::default())),
            "<T as Default>::default()"
        );
        assert_eq!(compact(quote!(x as u64)), "x as u64");
        assert_eq!(compact(quote!(x as Vec<u8>)), "x as Vec<u8>");
        assert_eq!(compact(quote!(Option<&str>)), "Option<&str>");
    }

    #[test]
    fn comparison_between_locals_or_constants_keeps_spaces() {
        assert_eq!(compact(quote!(a < b && c >= d)), "a < b && c >= d");
        assert_eq!(
            compact(quote!({ LIMIT < n && n > 10 })),
            "{ LIMIT < n && n > 10 }"
        );
    }

    #[test]
    fn string_literal_across_source_lines_prints_on_one_line() {
        let tokens = "\"first\nsecond\"".parse().expect("test source tokenizes");
        assert_eq!(compact(tokens), "\"first\\nsecond\"");
    }

    #[test]
    fn keywords_are_followed_by_a_space_before_delimiters() {
        assert_eq!(
            compact(quote!(if (a) { b } else { c })),
            "if (a) { b } else { c }"
        );
        assert_eq!(compact(quote!(return (x))), "return (x)");
    }

    #[test]
    fn try_and_await_bind_to_their_operand() {
        assert_eq!(compact(quote!(f(x)?.g().await?)), "f(x)?.g().await?");
    }

    #[test]
    fn attributes_bind_to_their_brackets() {
        assert_eq!(
            compact(quote!(
                #[cfg(test)]
                x
            )),
            "#[cfg(test)] x"
        );
    }
}
