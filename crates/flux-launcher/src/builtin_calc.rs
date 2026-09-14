use flux_core::{ResultKind, ResultSource, SearchResult};

use super::builtin::{BuiltinAction, BuiltinProvider, BuiltinQuery, BuiltinResult};

pub(crate) struct CalculatorProvider;

impl BuiltinProvider for CalculatorProvider {
    fn query(&self, request: &BuiltinQuery) -> Vec<BuiltinResult> {
        let expression = request.query.trim();
        if expression.len() > CALCULATOR_MAX_QUERY_LENGTH
            || !expression.chars().any(is_calculator_operator)
        {
            return Vec::new();
        }
        let Ok(value) = evaluate_expression(expression) else {
            return Vec::new();
        };
        let formatted = format_calculator_value(value);
        vec![BuiltinResult {
            result: SearchResult {
                id: String::from("builtin:calculator"),
                title: format!("= {formatted}"),
                subtitle: format!("Calculator • {expression}"),
                kind: ResultKind::Placeholder,
                source: ResultSource::Plugin,
                target: None,
            },
            action: Some(BuiltinAction::CopyText(formatted)),
        }]
    }
}

const CALCULATOR_MAX_QUERY_LENGTH: usize = 128;

fn is_calculator_operator(character: char) -> bool {
    matches!(character, '+' | '-' | '*' | '/' | '%' | '^' | '(' | ')')
}

fn evaluate_expression(expression: &str) -> Result<f64, ()> {
    let mut parser = CalculatorParser::new(expression);
    let value = parser.parse_expression()?;
    if parser.has_remaining_input() || !value.is_finite() {
        return Err(());
    }
    Ok(if value == 0.0 { 0.0 } else { value })
}

fn format_calculator_value(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0 {
        return format!("{}", value as i64);
    }
    let mut formatted = format!("{value:.12}");
    while formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

struct CalculatorParser {
    characters: Vec<char>,
    position: usize,
}

impl CalculatorParser {
    fn new(expression: &str) -> Self {
        Self {
            characters: expression.chars().collect(),
            position: 0,
        }
    }

    fn parse_expression(&mut self) -> Result<f64, ()> {
        self.parse_add_subtract()
    }

    fn parse_add_subtract(&mut self) -> Result<f64, ()> {
        let mut value = self.parse_multiply_divide()?;
        loop {
            if self.consume('+') {
                value += self.parse_multiply_divide()?;
            } else if self.consume('-') {
                value -= self.parse_multiply_divide()?;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_multiply_divide(&mut self) -> Result<f64, ()> {
        let mut value = self.parse_unary()?;
        loop {
            if self.consume('*') {
                value *= self.parse_unary()?;
            } else if self.consume('/') {
                let divisor = self.parse_unary()?;
                if divisor == 0.0 {
                    return Err(());
                }
                value /= divisor;
            } else if self.consume('%') {
                let divisor = self.parse_unary()?;
                if divisor == 0.0 {
                    return Err(());
                }
                value %= divisor;
            } else {
                return Ok(value);
            }
        }
    }

    fn parse_unary(&mut self) -> Result<f64, ()> {
        if self.consume('+') {
            return self.parse_unary();
        }
        if self.consume('-') {
            return Ok(-self.parse_unary()?);
        }
        self.parse_power()
    }

    fn parse_power(&mut self) -> Result<f64, ()> {
        let base = self.parse_primary()?;
        if self.consume('^') {
            let exponent = self.parse_unary()?;
            let value = base.powf(exponent);
            if !value.is_finite() {
                return Err(());
            }
            return Ok(value);
        }
        Ok(base)
    }

    fn parse_primary(&mut self) -> Result<f64, ()> {
        self.skip_whitespace();
        if self.consume('(') {
            let value = self.parse_add_subtract()?;
            if !self.consume(')') {
                return Err(());
            }
            return Ok(value);
        }
        let start = self.position;
        let mut digits = 0_usize;
        let mut dots = 0_usize;
        while let Some(character) = self.characters.get(self.position).copied() {
            if character.is_ascii_digit() {
                digits += 1;
                self.position += 1;
            } else if character == '.' {
                dots += 1;
                if dots > 1 {
                    return Err(());
                }
                self.position += 1;
            } else {
                break;
            }
        }
        if digits == 0 {
            return Err(());
        }
        self.characters[start..self.position]
            .iter()
            .collect::<String>()
            .parse::<f64>()
            .map_err(|_| ())
    }

    fn consume(&mut self, expected: char) -> bool {
        self.skip_whitespace();
        if self.characters.get(self.position) == Some(&expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn skip_whitespace(&mut self) {
        while self
            .characters
            .get(self.position)
            .is_some_and(|character| character.is_whitespace())
        {
            self.position += 1;
        }
    }

    fn has_remaining_input(&mut self) -> bool {
        self.skip_whitespace();
        self.position != self.characters.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator_provider_returns_flow_style_result_and_copy_action() {
        let request = BuiltinQuery {
            query: String::from("1 + 2 * 3"),
            google_enabled: false,
            google_keyword: String::from("g"),
            obsidian_enabled: false,
            obsidian_keyword: String::from("ob"),
        };
        let results = CalculatorProvider.query(&request);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result.title, "= 7");
        assert_eq!(results[0].result.subtitle, "Calculator • 1 + 2 * 3");
        assert!(matches!(
            &results[0].action,
            Some(BuiltinAction::CopyText(value)) if value == "7"
        ));
    }

    #[test]
    fn calculator_supports_parentheses_unary_signs_and_decimals() {
        assert_eq!(evaluate_expression("-(2 + 3) * 0.5"), Ok(-2.5));
        assert_eq!(evaluate_expression("2^3^2"), Ok(512.0));
        assert_eq!(format_calculator_value(10.0 / 3.0), "3.333333333333");
    }

    #[test]
    fn calculator_date_like_queries_are_explicitly_covered_for_policy_review() {
        let results = CalculatorProvider.query(&BuiltinQuery {
            query: String::from("2026-08"),
            google_enabled: false,
            google_keyword: String::from("g"),
            obsidian_enabled: false,
            obsidian_keyword: String::from("ob"),
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].result.title, "= 2018");
        assert_eq!(results[0].result.subtitle, "Calculator • 2026-08");
    }

    #[test]
    fn calculator_subtraction_remains_supported() {
        let results = CalculatorProvider.query(&BuiltinQuery {
            query: String::from("1-1"),
            google_enabled: false,
            google_keyword: String::from("g"),
            obsidian_enabled: false,
            obsidian_keyword: String::from("ob"),
        });
        assert_eq!(results[0].result.title, "= 0");
    }

    #[test]
    fn calculator_rejects_invalid_or_unsafe_input() {
        assert!(evaluate_expression("1 / 0").is_err());
        assert!(evaluate_expression("1 +").is_err());
        assert!(evaluate_expression("1 + process").is_err());
        assert!(CalculatorProvider
            .query(&BuiltinQuery {
                query: String::from("hello"),
                google_enabled: false,
                google_keyword: String::from("g"),
                obsidian_enabled: false,
                obsidian_keyword: String::from("ob"),
            })
            .is_empty());
    }
}
