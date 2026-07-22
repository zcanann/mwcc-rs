//! Integer expression evaluation for conditional preprocessing directives.

use std::collections::HashMap;

pub(super) fn evaluate(expression: &str, definitions: &HashMap<String, String>) -> bool {
    let mut parser = Parser {
        lexer: Lexer::new(expression),
        lookahead: None,
        definitions,
    };
    parser.parse_logical_or() != 0
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    lookahead: Option<Token>,
    definitions: &'a HashMap<String, String>,
}

impl Parser<'_> {
    fn parse_logical_or(&mut self) -> i64 {
        let mut value = self.parse_logical_and();
        while self.consume(Token::LogicalOr) {
            let right = self.parse_logical_and();
            value = i64::from(value != 0 || right != 0);
        }
        value
    }

    fn parse_logical_and(&mut self) -> i64 {
        let mut value = self.parse_bitwise_or();
        while self.consume(Token::LogicalAnd) {
            let right = self.parse_bitwise_or();
            value = i64::from(value != 0 && right != 0);
        }
        value
    }

    fn parse_bitwise_or(&mut self) -> i64 {
        let mut value = self.parse_bitwise_xor();
        while self.consume(Token::Pipe) {
            value |= self.parse_bitwise_xor();
        }
        value
    }

    fn parse_bitwise_xor(&mut self) -> i64 {
        let mut value = self.parse_bitwise_and();
        while self.consume(Token::Caret) {
            value ^= self.parse_bitwise_and();
        }
        value
    }

    fn parse_bitwise_and(&mut self) -> i64 {
        let mut value = self.parse_equality();
        while self.consume(Token::Ampersand) {
            value &= self.parse_equality();
        }
        value
    }

    fn parse_equality(&mut self) -> i64 {
        let mut value = self.parse_relation();
        loop {
            if self.consume(Token::Equal) {
                value = i64::from(value == self.parse_relation());
            } else if self.consume(Token::NotEqual) {
                value = i64::from(value != self.parse_relation());
            } else {
                return value;
            }
        }
    }

    fn parse_relation(&mut self) -> i64 {
        let mut value = self.parse_shift();
        loop {
            if self.consume(Token::LessEqual) {
                value = i64::from(value <= self.parse_shift());
            } else if self.consume(Token::GreaterEqual) {
                value = i64::from(value >= self.parse_shift());
            } else if self.consume(Token::Less) {
                value = i64::from(value < self.parse_shift());
            } else if self.consume(Token::Greater) {
                value = i64::from(value > self.parse_shift());
            } else {
                return value;
            }
        }
    }

    fn parse_shift(&mut self) -> i64 {
        let mut value = self.parse_sum();
        loop {
            if self.consume(Token::ShiftLeft) {
                value = value.wrapping_shl(self.parse_sum() as u32);
            } else if self.consume(Token::ShiftRight) {
                value = value.wrapping_shr(self.parse_sum() as u32);
            } else {
                return value;
            }
        }
    }

    fn parse_sum(&mut self) -> i64 {
        let mut value = self.parse_product();
        loop {
            if self.consume(Token::Plus) {
                value = value.wrapping_add(self.parse_product());
            } else if self.consume(Token::Minus) {
                value = value.wrapping_sub(self.parse_product());
            } else {
                return value;
            }
        }
    }

    fn parse_product(&mut self) -> i64 {
        let mut value = self.parse_unary();
        loop {
            if self.consume(Token::Star) {
                value = value.wrapping_mul(self.parse_unary());
            } else if self.consume(Token::Slash) {
                let divisor = self.parse_unary();
                value = if divisor == 0 { 0 } else { value / divisor };
            } else if self.consume(Token::Percent) {
                let divisor = self.parse_unary();
                value = if divisor == 0 { 0 } else { value % divisor };
            } else {
                return value;
            }
        }
    }

    fn parse_unary(&mut self) -> i64 {
        if self.consume(Token::Bang) {
            return i64::from(self.parse_unary() == 0);
        }
        if self.consume(Token::Tilde) {
            return !self.parse_unary();
        }
        if self.consume(Token::Minus) {
            return self.parse_unary().wrapping_neg();
        }
        if self.consume(Token::Plus) {
            return self.parse_unary();
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> i64 {
        match self.next() {
            Token::Integer(value) => value,
            Token::Identifier(name) if name == "defined" => {
                let parenthesized = self.consume(Token::ParenthesisOpen);
                let name = match self.next() {
                    Token::Identifier(name) => name,
                    _ => return 0,
                };
                if parenthesized {
                    self.consume(Token::ParenthesisClose);
                }
                i64::from(self.definitions.contains_key(&name))
            }
            Token::Identifier(name) => self.macro_value(&name),
            Token::ParenthesisOpen => {
                let value = self.parse_logical_or();
                self.consume(Token::ParenthesisClose);
                value
            }
            _ => 0,
        }
    }

    fn macro_value(&self, name: &str) -> i64 {
        let Some(value) = self.definitions.get(name) else {
            return 0;
        };
        parse_integer(value.trim()).unwrap_or(1)
    }

    fn peek(&mut self) -> Token {
        self.lookahead
            .get_or_insert_with(|| self.lexer.next())
            .clone()
    }

    fn next(&mut self) -> Token {
        self.lookahead.take().unwrap_or_else(|| self.lexer.next())
    }

    fn consume(&mut self, token: Token) -> bool {
        if self.peek() == token {
            self.next();
            true
        } else {
            false
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Token {
    Integer(i64),
    Identifier(String),
    ParenthesisOpen,
    ParenthesisClose,
    Bang,
    Tilde,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Equal,
    NotEqual,
    ShiftLeft,
    ShiftRight,
    Ampersand,
    Caret,
    Pipe,
    LogicalAnd,
    LogicalOr,
    End,
}

struct Lexer<'a> {
    source: &'a [u8],
    position: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            position: 0,
        }
    }

    fn next(&mut self) -> Token {
        while self
            .source
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
        let Some(&byte) = self.source.get(self.position) else {
            return Token::End;
        };
        if byte.is_ascii_digit() {
            let start = self.position;
            self.position += 1;
            while self.source.get(self.position).is_some_and(|byte| {
                byte.is_ascii_hexdigit() || matches!(byte, b'x' | b'X' | b'u' | b'U' | b'l' | b'L')
            }) {
                self.position += 1;
            }
            let text = std::str::from_utf8(&self.source[start..self.position]).unwrap_or("0");
            return Token::Integer(parse_integer(text).unwrap_or(0));
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = self.position;
            self.position += 1;
            while self
                .source
                .get(self.position)
                .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            {
                self.position += 1;
            }
            return Token::Identifier(
                std::str::from_utf8(&self.source[start..self.position])
                    .unwrap_or_default()
                    .to_string(),
            );
        }
        self.position += 1;
        let following = self.source.get(self.position).copied();
        let paired = match (byte, following) {
            (b'&', Some(b'&')) => Some(Token::LogicalAnd),
            (b'|', Some(b'|')) => Some(Token::LogicalOr),
            (b'=', Some(b'=')) => Some(Token::Equal),
            (b'!', Some(b'=')) => Some(Token::NotEqual),
            (b'<', Some(b'=')) => Some(Token::LessEqual),
            (b'>', Some(b'=')) => Some(Token::GreaterEqual),
            (b'<', Some(b'<')) => Some(Token::ShiftLeft),
            (b'>', Some(b'>')) => Some(Token::ShiftRight),
            _ => None,
        };
        if let Some(token) = paired {
            self.position += 1;
            return token;
        }
        match byte {
            b'(' => Token::ParenthesisOpen,
            b')' => Token::ParenthesisClose,
            b'!' => Token::Bang,
            b'~' => Token::Tilde,
            b'+' => Token::Plus,
            b'-' => Token::Minus,
            b'*' => Token::Star,
            b'/' => Token::Slash,
            b'%' => Token::Percent,
            b'<' => Token::Less,
            b'>' => Token::Greater,
            b'&' => Token::Ampersand,
            b'^' => Token::Caret,
            b'|' => Token::Pipe,
            _ => Token::End,
        }
    }
}

fn parse_integer(text: &str) -> Option<i64> {
    let text = text.trim_end_matches(|character| matches!(character, 'u' | 'U' | 'l' | 'L'));
    if let Some(hexadecimal) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        i64::from_str_radix(hexadecimal, 16).ok()
    } else if text.starts_with('0') && text.len() > 1 {
        i64::from_str_radix(&text[1..], 8).ok()
    } else {
        text.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate;
    use std::collections::HashMap;

    #[test]
    fn evaluates_defined_boolean_and_integer_relations() {
        let definitions = HashMap::from([
            (String::from("VERSION"), String::from("2")),
            (String::from("FEATURE"), String::from("1")),
        ]);
        assert!(evaluate(
            "defined(VERSION) && VERSION >= 2 && !defined(ABSENT)",
            &definitions
        ));
        assert!(evaluate("defined FEATURE || 0", &definitions));
        assert!(!evaluate("VERSION == 1 || ABSENT", &definitions));
    }

    #[test]
    fn observes_arithmetic_bitwise_and_suffixes() {
        let definitions = HashMap::new();
        assert!(evaluate("((1 << 4) | 3) == 0x13UL", &definitions));
        assert!(evaluate("010 + 2 == 10", &definitions));
    }
}
