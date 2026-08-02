use chrono::{DateTime, Utc};
use chrono::NaiveDateTime;

fn main() {
    // Test timestamp parsing
    let input = "2024-07-06 13:18:55";

    // Try parsing as NaiveDateTime
    if let Ok(dt) = NaiveDateTime::parse_from_str(input, "%Y-%m-%d %H:%M:%S") {
        let utc_dt = DateTime::from_naive_utc_and_offset(dt, Utc);
        println!("Input: {}", input);
        println!("Parsed as naive: {:?}", dt);
        println!("As UTC: {:?}", utc_dt);
        println!("Timestamp: {}", utc_dt.timestamp());
    }

    // Expected values from test
    println!("\nExpected timestamps from test:");
    println!("First:  1720267230 ({})", DateTime::from_timestamp(1720267230, 0).map(|d| d.format("%Y-%m-%d %H:%M:%S %Z").to_string()));
    println!("Second: 1720270345 ({})", DateTime::from_timestamp(1720270345, 0).map(|d| d.format("%Y-%m-%d %H:%M:%S %Z").to_string()));
    println!("Third:  1720272135 ({})", DateTime::from_timestamp(1720272135, 0).map(|d| d.format("%Y-%m-%d %H:%M:%S %Z").to_string()));

    // Actual values from input history
    println!("\nActual values from input history file:");
    println!("First:  2024-07-06 12:00:30 -> {}", NaiveDateTime::parse_from_str("2024-07-06 12:00:30", "%Y-%m-%d %H:%M:%S").map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc).timestamp()).unwrap());
    println!("Second: 2024-07-06 12:52:25 -> {}", NaiveDateTime::parse_from_str("2024-07-06 12:52:25", "%Y-%m-%d %H:%M:%S").map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc).timestamp()).unwrap());
    println!("Third:  2024-07-06 13:18:55 -> {}", NaiveDateTime::parse_from_str("2024-07-06 13:18:55", "%Y-%m-%d %H:%M:%S").map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc).timestamp()).unwrap());

    // What the test is actually getting
    println!("\nWhat the test is actually getting:");
    println!("Third:  1720271935 ({})", DateTime::from_timestamp(1720271935, 0).map(|d| d.format("%Y-%m-%d %H:%M:%S %Z").to_string()));

    // Difference
    println!("\nDifferences:");
    println!("Expected vs input history: {} seconds", 1720272135 - 1720272135);
    println!("Actual vs input history: {} seconds", 1720271935 - 1720272135);
}
