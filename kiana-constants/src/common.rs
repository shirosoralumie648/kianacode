use chrono::{Local, TimeZone};

pub fn get_local_iso_date() -> String {
    if let Ok(override_date) = std::env::var("CLAUDE_CODE_OVERRIDE_DATE") {
        return override_date;
    }
    Local::now().format("%Y-%m-%d").to_string()
}

pub fn get_local_month_year() -> String {
    let date = if let Ok(override_date) = std::env::var("CLAUDE_CODE_OVERRIDE_DATE") {
        chrono::NaiveDate::parse_from_str(&override_date, "%Y-%m-%d")
            .ok()
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .and_then(|dt| Local.from_local_datetime(&dt).single())
            .unwrap_or_else(Local::now)
    } else {
        Local::now()
    };
    date.format("%B %Y").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_local_iso_date() {
        let date = get_local_iso_date();
        assert!(date.len() == 10);
        assert!(date.contains('-'));
    }

    #[test]
    fn test_get_local_month_year() {
        let month_year = get_local_month_year();
        assert!(month_year.contains(' '));
    }
}
