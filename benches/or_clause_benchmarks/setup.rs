use super::*;

pub(super) fn init_database() {
    init_postgres_database(
        &DB_INITIALIZED,
        &[
            "DROP TABLE IF EXISTS or_bench_users CASCADE",
            r#"
                CREATE TABLE or_bench_users (
                    id BIGSERIAL PRIMARY KEY,
                    name VARCHAR(255) NOT NULL,
                    email VARCHAR(255) NOT NULL,
                    status VARCHAR(50) NOT NULL,
                    role VARCHAR(50) NOT NULL,
                    department VARCHAR(100) NOT NULL,
                    age INTEGER NOT NULL,
                    active BOOLEAN NOT NULL DEFAULT true
                )
            "#,
            "CREATE INDEX idx_or_bench_status ON or_bench_users(status)",
            "CREATE INDEX idx_or_bench_role ON or_bench_users(role)",
            "CREATE INDEX idx_or_bench_department ON or_bench_users(department)",
            "CREATE INDEX idx_or_bench_age ON or_bench_users(age)",
            "CREATE INDEX idx_or_bench_active ON or_bench_users(active)",
        ],
    );
}

pub(super) fn cleanup_data() {
    truncate_table("or_bench_users");
}

pub(super) fn reset_data(count: usize) {
    cleanup_data();
    seed_data(count);
}

pub(super) fn setup_benchmark_with_data(count: usize) {
    init_database();
    reset_data(count);
}

pub(super) fn seed_data(count: usize) {
    let rt = runtime();
    let statuses = ["active", "pending", "inactive", "banned"];
    let roles = ["admin", "moderator", "editor", "user", "guest"];
    let departments = ["Engineering", "Marketing", "Sales", "Support", "HR"];

    rt.block_on(async {
        let mut users = Vec::with_capacity(count);
        for i in 0..count {
            users.push(OrBenchUser {
                id: 0,
                name: format!("User {}", i),
                email: format!("user{}@example.com", i),
                status: statuses[i % statuses.len()].to_string(),
                role: roles[i % roles.len()].to_string(),
                department: departments[i % departments.len()].to_string(),
                age: 20 + (i % 50) as i32,
                active: i % 3 != 0,
            });
        }

        // Batch insert
        let _ = OrBenchUser::insert_all(users).await;
    });
}
