use super::{Backend, OrmBackend};

#[test]
fn known_engine_backends_map_without_guessing() {
    assert_eq!(
        Backend::from_orm_backend(OrmBackend::Postgres),
        Some(Backend::Postgres)
    );
    assert_eq!(
        Backend::from_orm_backend(OrmBackend::MySql),
        Some(Backend::MySql)
    );
    assert_eq!(
        Backend::from_orm_backend(OrmBackend::Sqlite),
        Some(Backend::Sqlite)
    );
}

#[test]
fn every_backend_round_trips_through_the_engine_enum() {
    for backend in [Backend::Postgres, Backend::MySql, Backend::Sqlite] {
        assert_eq!(Backend::from(OrmBackend::from(backend)), backend);
        assert_eq!(
            Backend::from_orm_backend(OrmBackend::from(backend)),
            Some(backend)
        );
    }
}
