use evalexpr::eval;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SearchResultItem {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub category: String,
    pub action_payload: String,
}

pub fn evaluate_conversions(query: &str) -> Option<SearchResultItem> {
    let query = query.trim().to_lowercase();

    // Math stuff
    if let Some(expr) = query.strip_prefix('=') {
        let expr = expr.trim();

        if expr.is_empty() {
            return Some(SearchResultItem {
                id: "math_error".into(),
                title: "Enter a mathematical expression".into(),
                subtitle: "Example: = 5 * 10".into(),
                category: "conversion".into(),
                action_payload: String::new(),
            });
        }

        match eval(expr) {
            Ok(result) => {
                return Some(SearchResultItem {
                    id: "math_result".into(),
                    title: format!("= {result}"),
                    subtitle: "Result ready (Press Enter to copy)".into(),
                    category: "conversion".into(),
                    action_payload: result.to_string(),
                });
            }

            Err(_) => {
                return Some(SearchResultItem {
                    id: "math_error".into(),
                    title: "Invalid mathematical expression".into(),
                    subtitle: "Example: = 5 * 10".into(),
                    category: "conversion".into(),
                    action_payload: String::new(),
                });
            }
        }
    }

    // Unit conversions (i hope i put them in right)
    let parts: Vec<&str> = query.split_whitespace().collect();

    if parts.len() == 2 {
        if let Ok(value) = parts[0].parse::<f64>() {
            let unit = parts[1];

            let (converted_value, target_unit) = match unit {
                "ft" | "feet" => (value * 0.3048, "meters (m)"),

                "m" | "meter" | "meters" => (value * 3.28084, "feet (ft)"),

                "in" | "inch" | "inches" => (value * 2.54, "centimeters (cm)"),

                "cm" | "centimeter" | "centimeters" => (value / 2.54, "inches (in)"),

                "mi" | "mile" | "miles" => (value * 1.60934, "kilometers (km)"),

                "km" | "kilometer" | "kilometers" => (value / 1.60934, "miles (mi)"),

                "lb" | "lbs" | "pound" | "pounds" => (value * 0.453592, "kilograms (kg)"),

                "kg" | "kilogram" | "kilograms" => (value * 2.20462, "pounds (lbs)"),

                "c" | "°c" | "celsius" => (value * 9.0 / 5.0 + 32.0, "Fahrenheit (°F)"),

                "f" | "°f" | "fahrenheit" => ((value - 32.0) * 5.0 / 9.0, "Celsius (°C)"),

                "px" => (value / 16.0, "rem (16px base)"),

                "rem" => (value * 16.0, "pixels (px)"),

                _ => return None,
            };

            return Some(SearchResultItem {
                id: format!("conv_{}", unit),

                title: format!("{:.4} {}", converted_value, target_unit),

                subtitle: format!("Converted from {} {}", value, unit),

                category: "conversion".into(),

                action_payload: format!("{:.4}", converted_value),
            });
        }
    }

    None
}
