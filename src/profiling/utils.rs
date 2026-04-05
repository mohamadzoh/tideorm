pub(crate) fn detect_operation(sql: &str) -> String {
    let sql_upper = sql.trim().to_uppercase();
    if sql_upper.starts_with("SELECT") {
        "SELECT".to_string()
    } else if sql_upper.starts_with("INSERT") {
        "INSERT".to_string()
    } else if sql_upper.starts_with("UPDATE") {
        "UPDATE".to_string()
    } else if sql_upper.starts_with("DELETE") {
        "DELETE".to_string()
    } else {
        "OTHER".to_string()
    }
}

pub(crate) fn textwrap_simple(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.is_empty() {
            current_line = word.to_string();
        } else if current_line.len() + 1 + word.len() <= width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}
