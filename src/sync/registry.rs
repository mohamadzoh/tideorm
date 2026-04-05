use super::*;

/// Global registry of entity registration functions
static ENTITY_REGISTRY: OnceLock<RwLock<Vec<EntityRegistrationFn>>> = OnceLock::new();

/// Registry of TideORM model schemas used during synchronization.
static MODEL_SCHEMAS: OnceLock<RwLock<Vec<ModelSchema>>> = OnceLock::new();

pub(super) fn get_entity_registry() -> &'static RwLock<Vec<EntityRegistrationFn>> {
    ENTITY_REGISTRY.get_or_init(|| RwLock::new(Vec::new()))
}

pub(super) fn get_model_schemas() -> &'static RwLock<Vec<ModelSchema>> {
    MODEL_SCHEMAS.get_or_init(|| RwLock::new(Vec::new()))
}

/// Trait for models that can be synced with the database
///
/// This trait is automatically implemented by TideORM's model macros.
/// Models that implement this trait can be registered for schema synchronization.
///
/// For TideORM models, this uses `ModelSchema` to define the table structure.
/// For entity types, you can use `SyncRegistry::register_entity::<E>()` directly.
pub trait SyncModel {
    /// Get the schema for this model
    fn sync_schema() -> ModelSchema;

    /// Register this model for synchronization
    fn register_for_sync() {
        SyncRegistry::register_schema(Self::sync_schema());
    }
}

/// Trait for registering multiple models at once
///
/// This is implemented for tuples of up to 200 model types.
/// Used by `TideConfig::models::<(Model1, Model2, ...)>()`.
pub trait RegisterModels {
    /// Register all models in this tuple
    fn register_all();
}

// Implement RegisterModels for empty tuple
impl RegisterModels for () {
    fn register_all() {}
}

/// Generate `RegisterModels` for tuples of size 1..N
macro_rules! impl_register_models_tuples {
    // Base case: single type
    ($first:ident) => {
        impl<$first: SyncModel> RegisterModels for ($first,) {
            fn register_all() {
                $first::register_for_sync();
            }
        }
    };
    // Recursive case: first + rest
    ($first:ident, $($rest:ident),+) => {
        // Recurse for smaller tuples first
        impl_register_models_tuples!($($rest),+);

        impl<$first: SyncModel, $($rest: SyncModel),+> RegisterModels for ($first, $($rest),+) {
            fn register_all() {
                $first::register_for_sync();
                $($rest::register_for_sync();)+
            }
        }
    };
}

// Generate impls for tuples of 1 through 200 elements
impl_register_models_tuples!(
    T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, T17, T18, T19, T20, T21,
    T22, T23, T24, T25, T26, T27, T28, T29, T30, T31, T32, T33, T34, T35, T36, T37, T38, T39, T40,
    T41, T42, T43, T44, T45, T46, T47, T48, T49, T50, T51, T52, T53, T54, T55, T56, T57, T58, T59,
    T60, T61, T62, T63, T64, T65, T66, T67, T68, T69, T70, T71, T72, T73, T74, T75, T76, T77, T78,
    T79, T80, T81, T82, T83, T84, T85, T86, T87, T88, T89, T90, T91, T92, T93, T94, T95, T96, T97,
    T98, T99, T100, T101, T102, T103, T104, T105, T106, T107, T108, T109, T110, T111, T112, T113,
    T114, T115, T116, T117, T118, T119, T120, T121, T122, T123, T124, T125, T126, T127, T128, T129,
    T130, T131, T132, T133, T134, T135, T136, T137, T138, T139, T140, T141, T142, T143, T144, T145,
    T146, T147, T148, T149, T150, T151, T152, T153, T154, T155, T156, T157, T158, T159, T160, T161,
    T162, T163, T164, T165, T166, T167, T168, T169, T170, T171, T172, T173, T174, T175, T176, T177,
    T178, T179, T180, T181, T182, T183, T184, T185, T186, T187, T188, T189, T190, T191, T192, T193,
    T194, T195, T196, T197, T198, T199, T200
);
