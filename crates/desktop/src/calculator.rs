//! Small arithmetic evaluator for the launcher's `=` input mode.
//!
//! Expressions are parsed directly, without a scripting runtime or external process.

const MAX_INPUT_BYTES: usize = 4096;
const MAX_DEPTH: usize = 64;

pub fn evaluate(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("请输入算式，例如 1 + 2 * 3".into());
    }
    if input.len() > MAX_INPUT_BYTES {
        return Err("算式过长，最多支持 4096 字节".into());
    }

    let mut parser = Parser { input, position: 0 };
    let value = parser.expression(0, 0)?;
    parser.skip_whitespace();
    if let Some(character) = parser.peek() {
        return Err(match character {
            ')' => "括号不匹配：存在多余的右括号".into(),
            '(' | '.' | '0'..='9' | 'a'..='z' | 'A'..='Z' | 'π' => "表达式缺少运算符".into(),
            _ => unsupported_character(character),
        });
    }

    Ok(format_result(value))
}

struct Parser<'a> {
    input: &'a str,
    position: usize,
}

impl Parser<'_> {
    fn expression(&mut self, minimum_precedence: u8, depth: usize) -> Result<f64, String> {
        if depth > MAX_DEPTH {
            return Err("算式嵌套过深，最多支持 64 层".into());
        }
        self.skip_whitespace();
        let mut value = match self.peek() {
            Some('+') => {
                self.position += 1;
                self.expression(5, depth + 1)?
            }
            Some('-') => {
                self.position += 1;
                -self.expression(5, depth + 1)?
            }
            Some('(') => {
                self.position += 1;
                let result = self.expression(0, depth + 1)?;
                self.skip_whitespace();
                if self.peek() != Some(')') {
                    return Err("括号不匹配：缺少右括号".into());
                }
                self.position += 1;
                result
            }
            Some('0'..='9' | '.') => self.number()?,
            Some('a'..='z' | 'A'..='Z' | 'π') => self.constant()?,
            Some(')') => return Err("算式不完整：右括号前缺少数值".into()),
            Some(character) => return Err(unsupported_character(character)),
            None => return Err("算式不完整：缺少数字或括号".into()),
        };

        loop {
            self.skip_whitespace();
            let (operator, left_precedence, right_precedence) = match self.peek() {
                Some('+') => ('+', 1, 2),
                Some('-') => ('-', 1, 2),
                Some('*') => ('*', 3, 4),
                Some('/') => ('/', 3, 4),
                Some('%') => ('%', 3, 4),
                // Exponentiation binds more tightly than unary signs and is right associative.
                Some('^') => ('^', 7, 6),
                _ => break,
            };
            if left_precedence < minimum_precedence {
                break;
            }
            self.position += 1;
            let right = self.expression(right_precedence, depth + 1)?;
            value = match operator {
                '+' => value + right,
                '-' => value - right,
                '*' => value * right,
                '/' => {
                    if right == 0.0 {
                        return Err("除数不能为零".into());
                    }
                    value / right
                }
                '%' => {
                    if right == 0.0 {
                        return Err("取余时除数不能为零".into());
                    }
                    value % right
                }
                '^' => value.powf(right),
                _ => unreachable!(),
            };
            check_finite(value)?;
        }

        Ok(value)
    }

    fn number(&mut self) -> Result<f64, String> {
        let start = self.position;
        let whole_digits = self.skip_digits();
        let fractional_digits = if self.peek() == Some('.') {
            self.position += 1;
            self.skip_digits()
        } else {
            0
        };
        if whole_digits + fractional_digits == 0 {
            return Err("小数点前后至少需要一个数字".into());
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.position += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.position += 1;
            }
            if self.skip_digits() == 0 {
                return Err("科学计数法不完整，例如 1e-3".into());
            }
        }
        let value = self.input[start..self.position]
            .parse::<f64>()
            .map_err(|_| "数字格式不正确".to_string())?;
        check_finite(value)?;
        Ok(value)
    }

    fn constant(&mut self) -> Result<f64, String> {
        if self.peek() == Some('π') {
            self.position += 'π'.len_utf8();
            return Ok(std::f64::consts::PI);
        }
        let start = self.position;
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_alphabetic())
        {
            self.position += 1;
        }
        let name = &self.input[start..self.position];
        if name.eq_ignore_ascii_case("pi") {
            Ok(std::f64::consts::PI)
        } else if name.eq_ignore_ascii_case("e") {
            Ok(std::f64::consts::E)
        } else {
            Err(format!("不支持的常量或函数：{name}；可用常量为 pi 和 e"))
        }
    }

    fn skip_digits(&mut self) -> usize {
        let start = self.position;
        while self
            .peek()
            .is_some_and(|character| character.is_ascii_digit())
        {
            self.position += 1;
        }
        self.position - start
    }

    fn skip_whitespace(&mut self) {
        while let Some(character) = self.peek().filter(|character| character.is_whitespace()) {
            self.position += character.len_utf8();
        }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }
}

fn unsupported_character(character: char) -> String {
    format!("不支持的字符：{character}")
}

fn check_finite(value: f64) -> Result<(), String> {
    if value.is_nan() {
        Err("计算结果不是有效实数".into())
    } else if !value.is_finite() {
        Err("数值超出可计算范围".into())
    } else {
        Ok(())
    }
}

fn format_result(value: f64) -> String {
    if value == 0.0 {
        return "0".into();
    }
    // Preserve exact integers within f64's reliable integer range. Other results use
    // 15 significant digits, which avoids displaying noise such as 0.30000000000000004.
    if value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_992.0 {
        return format!("{value:.0}");
    }
    let scientific = format!("{value:.14e}");
    let (mantissa, exponent) = scientific.split_once('e').expect("formatted exponent");
    let exponent = exponent.parse::<i32>().expect("formatted integer exponent");
    if (-6..=14).contains(&exponent) {
        let decimals = (14 - exponent) as usize;
        let fixed = format!("{value:.decimals$}");
        if decimals == 0 {
            fixed
        } else {
            fixed
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        }
    } else {
        format!(
            "{}e{exponent}",
            mantissa.trim_end_matches('0').trim_end_matches('.')
        )
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    #[test]
    fn respects_operator_precedence_and_right_associative_powers() {
        for (input, expected) in [
            ("2 + 3 * 4", "14"),
            ("(2 + 3) * 4", "20"),
            ("2 ^ 3 ^ 2", "512"),
            ("-2^2", "-4"),
            ("(-2)^2", "4"),
            ("2^-3", "0.125"),
            ("10 % 3", "1"),
            ("3 - -2", "5"),
        ] {
            assert_eq!(evaluate(input).unwrap(), expected, "{input}");
        }
    }

    #[test]
    fn formats_decimals_scientific_notation_and_constants() {
        for (input, expected) in [
            ("0.1 + 0.2", "0.3"),
            (".5 + 1.", "1.5"),
            ("1e-3 * 2E3", "2"),
            ("1 / 3", "0.333333333333333"),
            ("1e-10", "1e-10"),
            ("-0", "0"),
            ("2^53", "9007199254740992"),
            ("100000000000000.125", "100000000000000"),
            ("π - PI", "0"),
        ] {
            assert_eq!(evaluate(input).unwrap(), expected, "{input}");
        }
        assert!(evaluate("e").unwrap().starts_with("2.718281828459"));
    }

    #[test]
    fn rejects_incomplete_unsupported_and_invalid_arithmetic() {
        for input in [
            "",
            "1 +",
            "(1 + 2",
            "1)",
            "()",
            "1 2",
            "2pi",
            "1.2.3",
            "1e",
            "1e+",
            ".",
            "sqrt(4)",
            "1;2",
            "中文",
            "1/0",
            "3%0",
            "(-1)^0.5",
            "1e309",
            "1e308 * 10",
        ] {
            assert!(evaluate(input).is_err(), "{input}");
        }
    }

    #[test]
    fn bounds_input_length_and_recursive_expression_depth() {
        assert!(evaluate(&"1".repeat(4097)).is_err());
        assert!(evaluate(&format!("{}1{}", "(".repeat(65), ")".repeat(65))).is_err());
        assert!(evaluate(&format!("{}1", "-".repeat(65))).is_err());
        assert!(evaluate(&vec!["1"; 66].join("^")).is_err());
        assert_eq!(evaluate(&vec!["1"; 512].join("+")).unwrap(), "512");
    }
}
