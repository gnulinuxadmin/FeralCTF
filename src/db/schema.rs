// FeralCTF - Database Schema
// Implements FERALCTF_SPEC.md section 4

use rusqlite::Connection;

pub fn setup_database(_conn: &Connection) -> Result<(), anyhow::Error> {
    _conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT UNIQUE NOT NULL,
            password_hash TEXT NOT NULL,
            email TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )
        
        CREATE TABLE IF NOT EXISTS challenges (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE NOT NULL,
            category TEXT NOT NULL,
            points INTEGER NOT NULL,
            flag TEXT NOT NULL,
            description TEXT NOT NULL,
            hint TEXT,
            difficulty_level TEXT NOT NULL,
            active INTEGER DEFAULT 1,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )
        
        CREATE TABLE IF NOT EXISTS flags (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            challenge_id INTEGER NOT NULL,
            user_id INTEGER NOT NULL,
            flag_type TEXT NOT NULL,
            flag_value TEXT NOT NULL,
            is_correct INTEGER NOT NULL,
            solved_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (challenge_id) REFERENCES challenges(id),
            FOREIGN KEY (user_id) REFERENCES users(id)
        )
        
        CREATE TABLE IF NOT EXISTS teams (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT UNIQUE NOT NULL,
            university TEXT NOT NULL,
            members TEXT NOT NULL,
            captain_name TEXT,
            email TEXT,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
        )
        
        CREATE TABLE IF NOT EXISTS scores (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            team_id INTEGER NOT NULL,
            challenge_id INTEGER NOT NULL,
            points INTEGER NOT NULL,
            solved_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (team_id) REFERENCES teams(id),
            FOREIGN KEY (challenge_id) REFERENCES challenges(id)
        )
"
    ).map_err(|e| anyhow::anyhow!("Failed to create tables: {}", e))?;
    
    Ok(())
}

pub fn migrate_database(_conn: &Connection) -> Result<(), anyhow::Error> {
    // Stub implementation for now
    // Will be implemented when migration files are added
    Ok(())
}

pub fn create_tables(_conn: &Connection) -> Result<(), anyhow::Error> {
    // This is now done in setup_database
    Ok(())
}