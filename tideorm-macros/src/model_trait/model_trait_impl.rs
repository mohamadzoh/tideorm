use super::*;

pub(super) fn generate_model_trait_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let primary_key_impl = build_primary_key_value_impl(ctx);
    let table_name = &ctx.table_name;
    let pk_auto_increment = ctx.pk_auto_increment;
    let column_names = &ctx.column_names;
    let column_variants = &ctx.column_variants;
    let field_names = &ctx.field_names;
    let pk_contains_conflict_check = build_pk_conflict_check(ctx);
    let pk_exclusion_check = build_pk_exclusion_check(ctx, quote!(column));
    let encrypted_conflict_column_check = build_encrypted_conflict_column_check(ctx);

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::model::Model for #struct_name {
            fn primary_key(&self) -> Self::PrimaryKey {
                #primary_key_impl
            }

            async fn find(id: Self::PrimaryKey) -> ::tideorm::Result<Option<Self>> {
                use ::tideorm::database::Connection;
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::{EntityTrait, QueryFilter};
                let error_context = Self::__primary_key_error_context(&id)
                    .query(format!("find({})", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&id)));
                let result = ::tideorm::profiling::__profile_future(async move {
                    let connection = ::tideorm::database::__current_connection()
                        .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                    match connection {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::find()
                                .filter(<Self as InternalModel>::primary_key_condition(&id))
                                .one(conn.connection())
                                .await
                        }
                        ::tideorm::database::ConnectionRef::Transaction(tx) => {
                            #internal_entity_mod::Entity::find()
                                .filter(<Self as InternalModel>::primary_key_condition(&id))
                                .one(tx.as_ref())
                                .await
                        }
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                result
                    .map(<Self as InternalModel>::try_from_entity_model)
                    .transpose()
            }

            async fn find_with(
                id: Self::PrimaryKey,
                db: &::tideorm::database::Database,
            ) -> ::tideorm::Result<Option<Self>> {
                use ::tideorm::database::Connection;
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::{EntityTrait, QueryFilter};
                let error_context = Self::__primary_key_error_context(&id)
                    .query(format!("find({})", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&id)));
                let result = ::tideorm::profiling::__profile_future(async move {
                    let connection = db
                        .__get_connection()
                        .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                    match connection {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::find()
                                .filter(<Self as InternalModel>::primary_key_condition(&id))
                                .one(conn.connection())
                                .await
                        }
                        ::tideorm::database::ConnectionRef::Transaction(tx) => {
                            #internal_entity_mod::Entity::find()
                                .filter(<Self as InternalModel>::primary_key_condition(&id))
                                .one(tx.as_ref())
                                .await
                        }
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                result
                    .map(<Self as InternalModel>::try_from_entity_model)
                    .transpose()
            }

            async fn destroy(id: Self::PrimaryKey) -> ::tideorm::Result<u64> {
                use ::tideorm::database::Connection;
                use ::tideorm::orm::{EntityTrait, QueryFilter};
                let error_context = Self::__primary_key_error_context(&id)
                    .query(format!("destroy({})", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&id)));
                let dirty_tracking_id = if ::tideorm::model::__dirty_tracking_enabled() {
                    Some(id.clone())
                } else {
                    None
                };
                let result = ::tideorm::profiling::__profile_future(async move {
                    let connection = ::tideorm::database::__current_connection()
                        .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                    match connection {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::delete_many()
                                .filter(<Self as ::tideorm::internal::InternalModel>::primary_key_condition(&id))
                                .exec(conn.connection())
                                .await
                        }
                        ::tideorm::database::ConnectionRef::Transaction(tx) => {
                            #internal_entity_mod::Entity::delete_many()
                                .filter(<Self as ::tideorm::internal::InternalModel>::primary_key_condition(&id))
                                .exec(tx.as_ref())
                                .await
                        }
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                if result.rows_affected > 0 {
                    ::tideorm::QueryCache::global().invalidate_model(#table_name);
                    if let Some(dirty_tracking_id) = dirty_tracking_id.as_ref() {
                        let _ = ::tideorm::model::__forget_dirty_snapshot_by_pk::<Self>(dirty_tracking_id);
                    }
                }
                Ok(result.rows_affected)
            }

            async fn create(model: Self) -> ::tideorm::Result<Self> {
                use ::tideorm::database::Connection;
                use ::tideorm::callbacks::{
                    AfterCreateDispatch, AfterValidationDispatch, BeforeCreateOnlyDispatch,
                    BeforeSaveDispatch, BeforeValidationDispatch,
                };
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::ActiveModelTrait;
                use ::tideorm::validation::Validate;
                let mut model = model;
                (&mut model).run_before_validation()?;
                ::tideorm::validation::Validate::validate(&model)
                    .map_err(::tideorm::Error::from)?;
                (&model).run_after_validation()?;
                (&mut model).run_before_save()?;
                (&mut model).run_before_create_only()?;
                let error_context = Self::__base_error_context().query(format!("insert into {}", #table_name));
                let active = <Self as InternalModel>::try_into_active_model(model)?;
                let result = ::tideorm::profiling::__profile_future(
                    async move {
                        let connection = ::tideorm::database::__current_connection()
                            .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                        match connection {
                            ::tideorm::database::ConnectionRef::Database(conn) => active.insert(conn.connection()).await,
                            ::tideorm::database::ConnectionRef::Transaction(tx) => active.insert(tx.as_ref()).await,
                        }
                    }
                )
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                let model = <Self as InternalModel>::try_from_entity_model(result)?;
                ::tideorm::QueryCache::global().invalidate_model(#table_name);
                (&model).run_after_create()?;
                Ok(model)
            }

            async fn delete(self) -> ::tideorm::Result<u64> {
                use ::tideorm::database::Connection;
                use ::tideorm::orm::ActiveModelTrait;
                use ::tideorm::callbacks::{AfterDeleteDispatch, BeforeDeleteDispatch};
                let model = self;
                (&model).run_before_delete()?;
                let primary_key = model.primary_key();
                let error_context = Self::__primary_key_error_context(&primary_key)
                    .query(format!("delete where {}", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&primary_key)));
                let active = model.clone().__into_delete_active_model();
                let result = ::tideorm::profiling::__profile_future(async move {
                    let connection = ::tideorm::database::__current_connection()
                        .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                    match connection {
                        ::tideorm::database::ConnectionRef::Database(conn) => active.delete(conn.connection()).await,
                        ::tideorm::database::ConnectionRef::Transaction(tx) => active.delete(tx.as_ref()).await,
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                if result.rows_affected > 0 {
                    ::tideorm::QueryCache::global().invalidate_model(#table_name);
                    if ::tideorm::model::__dirty_tracking_enabled() {
                        let _ = ::tideorm::model::__forget_dirty_snapshot(&model);
                    }
                }
                (&model).run_after_delete()?;
                Ok(result.rows_affected)
            }

            async fn save(self) -> ::tideorm::Result<Self> {
                if self.is_new() {
                    Self::create(self).await
                } else {
                    let primary_key = self.primary_key();
                    if <Self as ::tideorm::model::ModelMeta>::primary_key_auto_increment()
                        && <Self as ::tideorm::model::ModelMeta>::primary_key_names().len() == 1
                    {
                        self.update().await
                    } else if Self::exists(primary_key).await? {
                        self.update().await
                    } else {
                        Self::create(self).await
                    }
                }
            }

            async fn update(self) -> ::tideorm::Result<Self> {
                use ::tideorm::database::Connection;
                use ::tideorm::callbacks::{
                    AfterUpdateDispatch, AfterValidationDispatch, BeforeSaveDispatch,
                    BeforeUpdateOnlyDispatch, BeforeValidationDispatch,
                };
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::ActiveModelTrait;
                use ::tideorm::validation::Validate;
                let mut model = self;
                (&mut model).run_before_validation()?;
                ::tideorm::validation::Validate::validate(&model)
                    .map_err(::tideorm::Error::from)?;
                (&model).run_after_validation()?;
                (&mut model).run_before_save()?;
                (&mut model).run_before_update_only()?;
                let primary_key = model.primary_key();
                let error_context = Self::__primary_key_error_context(&primary_key)
                    .query(format!("update where {}", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&primary_key)));
                let active = model.__into_update_active_model()?;
                let result = ::tideorm::profiling::__profile_future(
                    async move {
                        let connection = ::tideorm::database::__current_connection()
                            .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                        match connection {
                            ::tideorm::database::ConnectionRef::Database(conn) => active.update(conn.connection()).await,
                            ::tideorm::database::ConnectionRef::Transaction(tx) => active.update(tx.as_ref()).await,
                        }
                    }
                )
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                let model = <Self as InternalModel>::try_from_entity_model(result)?;
                ::tideorm::QueryCache::global().invalidate_model(#table_name);
                (&model).run_after_update()?;
                Ok(model)
            }

            async fn insert_or_update(model: Self, conflict_columns: Vec<&str>) -> ::tideorm::Result<Self> {
                let cols: Vec<String> = conflict_columns.into_iter().map(|value| value.to_string()).collect();
                let builder = ::tideorm::model::OnConflictBuilder::new(cols);
                Self::__insert_with_conflict(model, builder).await
            }

            async fn __insert_with_conflict(
                model: Self,
                builder: ::tideorm::model::OnConflictBuilder<Self>,
            ) -> ::tideorm::Result<Self> {
                use ::tideorm::database::Connection;
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter};
                use ::tideorm::orm::sea_query::OnConflict;

                let model_for_lookup = model.clone();
                let conflict_cols = builder.conflict_columns;
                #encrypted_conflict_column_check
                let include_pk = #pk_contains_conflict_check || !#pk_auto_increment;
                let insertable_columns: Vec<&str> = vec![#(#column_names),*]
                    .into_iter()
                    .filter(|column| !(*column == <Self as ::tideorm::model::ModelMeta>::primary_key_name() && #pk_auto_increment && !include_pk))
                    .collect();
                let conflict_columns: Vec<_> = conflict_cols
                    .iter()
                    .map(|column| {
                        Self::column_from_str(column).ok_or_else(|| {
                            ::tideorm::Error::invalid_query(format!(
                                "unknown conflict column '{}' for {}",
                                column,
                                #table_name
                            ))
                        })
                    })
                    .collect::<::tideorm::Result<Vec<_>>>()?;
                let update_cols: Vec<String> = if let Some(cols) = builder.update_columns {
                    cols
                } else if let Some(exclude) = builder.exclude_columns {
                    insertable_columns
                        .iter()
                        .filter(|column| !exclude.contains(&column.to_string()))
                        .map(|column| column.to_string())
                        .collect()
                } else {
                    insertable_columns.iter().filter(|column| {
                        let column = column.to_string();
                        !conflict_cols.contains(&column) && !#pk_exclusion_check
                    }).map(|column| column.to_string()).collect()
                };
                let update_columns: Vec<_> = update_cols
                    .iter()
                    .map(|column| {
                        Self::column_from_str(column).ok_or_else(|| {
                            ::tideorm::Error::invalid_query(format!(
                                "unknown update column '{}' for {}",
                                column,
                                #table_name
                            ))
                        })
                    })
                    .collect::<::tideorm::Result<Vec<_>>>()?;

                let on_conflict = if update_columns.is_empty() {
                    OnConflict::columns(conflict_columns.iter().cloned())
                        .do_nothing()
                        .to_owned()
                } else {
                    OnConflict::columns(conflict_columns.iter().cloned())
                        .update_columns(update_columns.iter().cloned())
                        .to_owned()
                };

                let sql = format!(
                    "insert_or_update into {} on conflict ({})",
                    #table_name,
                    conflict_cols.join(", ")
                );
                let error_context = Self::__base_error_context().query(sql.clone());

                let entity_model = <Self as InternalModel>::try_to_entity_model(&model)?;
                let active_model = if include_pk {
                    entity_model.into_active_model()
                } else {
                    <Self as InternalModel>::try_into_active_model(model)?
                };

                ::tideorm::profiling::__profile_future(async move {
                    let connection = ::tideorm::database::__current_connection()
                        .map_err(|error| ::tideorm::orm::OrmError::Custom(error.to_string()))?;
                    match connection {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::insert(active_model)
                                .on_conflict(on_conflict)
                                .exec(conn.connection())
                                .await
                        }
                        ::tideorm::database::ConnectionRef::Transaction(tx) => {
                            #internal_entity_mod::Entity::insert(active_model)
                                .on_conflict(on_conflict)
                                .exec(tx.as_ref())
                                .await
                        }
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context.clone()))?;
                ::tideorm::QueryCache::global().invalidate_model(#table_name);

                let mut finder = #internal_entity_mod::Entity::find();
                for conflict_column in &conflict_cols {
                    finder = match conflict_column.as_str() {
                        #(#column_names | stringify!(#field_names) => finder.filter(#internal_entity_mod::Column::#column_variants.eq(model_for_lookup.#field_names.clone())),)*
                        _ => {
                            return Err(::tideorm::Error::invalid_query(format!(
                                "unknown conflict column '{}' for {}",
                                conflict_column,
                                #table_name
                            )));
                        }
                    };
                }

                let result = match ::tideorm::database::__current_connection()? {
                    ::tideorm::database::ConnectionRef::Database(conn) => finder.one(conn.connection()).await,
                    ::tideorm::database::ConnectionRef::Transaction(tx) => finder.one(tx.as_ref()).await,
                }
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;

                result
                    .map(<Self as InternalModel>::try_from_entity_model)
                    .transpose()?
                    .ok_or_else(|| ::tideorm::Error::query("upsert completed but no matching row could be reloaded".to_string()))
            }
        }
    }
}

fn build_encrypted_conflict_column_check(ctx: &BuildContext) -> TokenStream2 {
    if ctx.encrypted_fields.is_empty() {
        return quote! {};
    }

    let encrypted_fields = &ctx.encrypted_fields;
    let encrypted_column_names = &ctx.encrypted_column_names;
    let table_name = &ctx.table_name;

    quote! {
        const __TIDEORM_ENCRYPTED_CONFLICT_COLUMNS: &[&str] = &[
            #(#encrypted_fields),*,
            #(#encrypted_column_names),*
        ];
        for conflict_column in &conflict_cols {
            if __TIDEORM_ENCRYPTED_CONFLICT_COLUMNS.contains(&conflict_column.as_str()) {
                return Err(::tideorm::Error::invalid_query(format!(
                    "encrypted field '{}' cannot be used as an insert_or_update conflict column for {}; encrypted fields use randomized ciphertext, so use a plaintext unique key instead",
                    conflict_column,
                    #table_name
                )));
            }
        }
    }
}
