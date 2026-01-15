//! Date/Time Types Demo
//!
//! This example demonstrates how to properly use date and time types in TideORM,
//! including the mapping between Rust types and SQL types.
//!
//! ## Key Concepts
//!
//! - `chrono::DateTime<Utc>` → TIMESTAMPTZ (PostgreSQL) / TIMESTAMP (MySQL)
//! - `chrono::NaiveDateTime` → TIMESTAMP (without timezone)
//! - `chrono::NaiveDate` → DATE
//! - `chrono::NaiveTime` → TIME
//!
//! ## Running the Example
//!
//! ```bash
//! # Set up a PostgreSQL database
//! createdb datetime_demo
//!
//! # Run the example
//! cargo run --example datetime_types_demo
//! ```

use tideorm::prelude::*;

// ============================================================================
// MODEL DEFINITIONS
// ============================================================================

/// User session with timezone-aware timestamps
/// 
/// Use `DateTime<Utc>` for timestamps that should track timezone information.
/// This maps to TIMESTAMPTZ in PostgreSQL.
#[tideorm::model]
#[tide(table = "sessions")]
pub struct Session {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub user_id: i64,
    pub token: String,
    
    /// When the session expires - uses TIMESTAMPTZ
    pub expires_at: chrono::DateTime<chrono::Utc>,
    
    /// Last activity time - nullable TIMESTAMPTZ
    pub last_activity_at: Option<chrono::DateTime<chrono::Utc>>,
    
    /// Standard timestamps - also use TIMESTAMPTZ
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Event with date-only field
/// 
/// Use `NaiveDate` when you only need the date without time.
/// This maps to DATE in all databases.
#[tideorm::model]
#[tide(table = "events")]
pub struct Event {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    
    /// Event date - uses DATE type
    pub event_date: chrono::NaiveDate,
    
    /// Optional end date
    pub end_date: Option<chrono::NaiveDate>,
    
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Daily schedule with time-only fields
/// 
/// Use `NaiveTime` when you only need time without date.
/// This maps to TIME in PostgreSQL/MySQL.
#[tideorm::model]
#[tide(table = "schedules")]
pub struct Schedule {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub name: String,
    
    /// Start time - uses TIME type
    pub start_time: chrono::NaiveTime,
    
    /// End time - uses TIME type  
    pub end_time: chrono::NaiveTime,
    
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Log entry with naive datetime (no timezone)
/// 
/// Use `NaiveDateTime` when timezone is not relevant (e.g., local time logs).
/// This maps to TIMESTAMP (without time zone) in PostgreSQL.
#[tideorm::model]
#[tide(table = "logs")]
pub struct Log {
    #[tide(primary_key, auto_increment)]
    pub id: i64,
    pub message: String,
    pub level: String,
    
    /// Local timestamp without timezone - uses TIMESTAMP
    pub logged_at: chrono::NaiveDateTime,
}

// ============================================================================
// MIGRATIONS
// ============================================================================

/// Migration demonstrating all date/time column types
#[derive(Default)]
struct CreateDateTimeTables;

#[async_trait]
impl Migration for CreateDateTimeTables {
    fn version(&self) -> &str {
        "20260115_001"
    }

    fn name(&self) -> &str {
        "create_datetime_tables"
    }

    async fn up(&self, schema: &mut Schema) -> tideorm::Result<()> {
        // Sessions table - uses TIMESTAMPTZ for DateTime<Utc>
        schema
            .create_table("sessions", |t| {
                t.id();
                t.big_integer("user_id").not_null();
                t.string("token").unique().not_null();
                
                // TIMESTAMPTZ - for DateTime<Utc>
                t.timestamptz("expires_at").not_null();
                t.timestamptz("last_activity_at").nullable();                
                // timestamps() now uses TIMESTAMPTZ by default
                t.timestamps();
            })
            .await?;

        // Events table - uses DATE for NaiveDate
        schema
            .create_table("events", |t| {
                t.id();
                t.string("name").not_null();
                
                // DATE - for NaiveDate
                t.date("event_date").not_null();
                t.date("end_date").nullable();
                
                t.timestamps();
            })
            .await?;

        // Schedules table - uses TIME for NaiveTime
        schema
            .create_table("schedules", |t| {
                t.id();
                t.string("name").not_null();
                
                // TIME - for NaiveTime
                t.time("start_time").not_null();
                t.time("end_time").not_null();
                
                t.timestamps();
            })
            .await?;

        // Logs table - uses TIMESTAMP (no timezone) for NaiveDateTime
        schema
            .create_table("logs", |t| {
                t.id();
                t.text("message").not_null();
                t.string("level").not_null();
                
                // TIMESTAMP (without timezone) - for NaiveDateTime
                t.timestamp("logged_at").default_now().not_null();
            })
            .await?;

        Ok(())
    }

    async fn down(&self, schema: &mut Schema) -> tideorm::Result<()> {
        schema.drop_table_if_exists("logs").await?;
        schema.drop_table_if_exists("schedules").await?;
        schema.drop_table_if_exists("events").await?;
        schema.drop_table_if_exists("sessions").await?;
        Ok(())
    }
}

// ============================================================================
// MAIN
// ============================================================================

#[tokio::main]
async fn main() -> tideorm::Result<()> {
    println!("=== TideORM Date/Time Types Demo ===\n");
 let _ = dotenvy::dotenv();
    let db_url = std::env::var("POSTGRESQL_DATABASE_URL").unwrap();
    
    // Connect to database
    TideConfig::init()
        .database(&db_url)
        .connect()
        .await?;

    println!("Connected to database.\n");

    // Run migrations
    println!("Running migrations...");
    Migrator::new()
        .add(CreateDateTimeTables::default())
        .run()
        .await?;
    println!("Migrations complete.\n");

    // ========================================================================
    // DEMO: DateTime<Utc> - TIMESTAMPTZ
    // ========================================================================
    println!("--- Demo: DateTime<Utc> (TIMESTAMPTZ) ---");
    
    let now = chrono::Utc::now();
    let session = Session {
        id: 0,
        user_id: 1,
        token: format!("token_{}", now.timestamp_millis()),
        expires_at: now + chrono::Duration::hours(24),
        last_activity_at: Some(now),
        created_at: now,
        updated_at: now,
    };
    let session = session.save().await?;
    println!("Created session with expires_at: {}", session.expires_at);
    println!("  - expires_at is stored as TIMESTAMPTZ in PostgreSQL");
    println!("  - Includes timezone information (UTC)\n");

    // ========================================================================
    // DEMO: NaiveDate - DATE
    // ========================================================================
    println!("--- Demo: NaiveDate (DATE) ---");
    
    let event = Event {
        id: 0,
        name: "Conference".to_string(),
        event_date: chrono::NaiveDate::from_ymd_opt(2026, 3, 15).unwrap(),
        end_date: Some(chrono::NaiveDate::from_ymd_opt(2026, 3, 17).unwrap()),
        created_at: now,
        updated_at: now,
    };
    let event = event.save().await?;
    println!("Created event on date: {}", event.event_date);
    println!("  - event_date is stored as DATE in PostgreSQL");
    println!("  - Only stores the date, no time component\n");

    // ========================================================================
    // DEMO: NaiveTime - TIME
    // ========================================================================
    println!("--- Demo: NaiveTime (TIME) ---");
    
    let schedule = Schedule {
        id: 0,
        name: "Morning Standup".to_string(),
        start_time: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        end_time: chrono::NaiveTime::from_hms_opt(9, 15, 0).unwrap(),
        created_at: now,
        updated_at: now,
    };
    let schedule = schedule.save().await?;
    println!("Created schedule: {} - {}", schedule.start_time, schedule.end_time);
    println!("  - start_time/end_time are stored as TIME in PostgreSQL");
    println!("  - Only stores the time, no date component\n");

    // ========================================================================
    // DEMO: NaiveDateTime - TIMESTAMP
    // ========================================================================
    println!("--- Demo: NaiveDateTime (TIMESTAMP) ---");
    
    let log = Log {
        id: 0,
        message: "Application started".to_string(),
        level: "INFO".to_string(),
        logged_at: chrono::Local::now().naive_local(),
    };
    let log = log.save().await?;
    println!("Created log at: {}", log.logged_at);
    println!("  - logged_at is stored as TIMESTAMP in PostgreSQL");
    println!("  - No timezone information (naive datetime)\n");

    // ========================================================================
    // QUERYING
    // ========================================================================
    println!("--- Querying ---");
    
    // Query all sessions for a user (integer comparison works fine)
    let user_sessions = Session::query()
        .where_eq("user_id", 1)
        .get()
        .await?;
    println!("Found {} sessions for user 1", user_sessions.len());

    // For date/time comparisons in PostgreSQL, use where_raw with proper SQL
    // This ensures PostgreSQL handles the type casting correctly
    
    // Query events in March 2026 using DATE literals
    let march_events = Event::query()
        .where_raw("event_date >= '2026-03-01'::date")
        .where_raw("event_date < '2026-04-01'::date")
        .get()
        .await?;
    println!("Found {} events in March 2026", march_events.len());

    // Query schedules starting before noon using TIME literal
    let morning_schedules = Schedule::query()
        .where_raw("start_time < '12:00:00'::time")
        .get()
        .await?;
    println!("Found {} morning schedules", morning_schedules.len());
    
    // Query active sessions using TIMESTAMPTZ comparison
    let active_sessions = Session::query()
        .where_raw("expires_at > NOW()")
        .get()
        .await?;
    println!("Found {} active sessions", active_sessions.len());

    println!("\n=== Demo Complete ===");
    println!("\nType Mapping Summary:");
    println!("  Rust Type              | PostgreSQL    | Use For");
    println!("  -----------------------|---------------|------------------");
    println!("  chrono::DateTime<Utc>  | TIMESTAMPTZ   | UTC timestamps");
    println!("  chrono::NaiveDateTime  | TIMESTAMP     | Local timestamps");
    println!("  chrono::NaiveDate      | DATE          | Date only");
    println!("  chrono::NaiveTime      | TIME          | Time only");

    Ok(())
}
