use super::*;

pub(super) fn generate_model_trait_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let primary_key_impl = build_primary_key_value_impl(ctx);
    let table_name = &ctx.table_name;
    let pk_auto_increment = ctx.pk_auto_increment;
    let column_names = &ctx.column_names;
    let column_variants = &ctx.column_variants;
    let field_idents = &ctx.field_idents;
    let name_patterns = build_name_patterns(ctx);
    let is_pk_column = build_is_pk_column(ctx, quote!(column));
    let encrypted_conflict_column_check = build_encrypted_conflict_column_check(ctx);
    let pk_idents = &ctx.pk_idents;
    // An upsert keys a nil `Uuid` key before copying the model it looks the
    // row up by afterwards; the insert conversion would key only the insert.
    let uuid_key_idents: Vec<_> = ctx
        .db_fields
        .iter()
        .filter(|field| field.primary_key && field.is_uuid())
        .map(|field| field.ident())
        .collect();
    let key_uuid_keys = (!uuid_key_idents.is_empty()).then(|| {
        quote! {
            let mut model = model;
            #(model.#uuid_key_idents = ::tideorm::model::__uuid_key(model.#uuid_key_idents);)*
        }
    });
    let managed_created_at_columns: Vec<String> = ctx
        .db_fields
        .iter()
        .filter(|field| crate::meta_support::is_managed_created_at(field))
        .map(|field| field.column_name())
        .collect();

    // `find` and `find_with` share an identical primary-key lookup; they differ
    // only in how the connection is acquired. Interpolate that one expression so
    // the shared body — including the `__profile_future` wrapper — stays single-source.
    // The connection is resolved before the profiled statement so a missing or
    // unreachable database keeps its `Error::Connection` class. A key matches at
    // most one row, so the lookup reads with `all()`: `one()` adds a bound
    // `LIMIT`, which recent SQLite releases recompile the statement for on every run.
    // The lookup keeps the soft-delete scope, as every other read does.
    let find_body = |connection_expr: TokenStream2| {
        quote! {
            use ::tideorm::orm::{EntityTrait, QueryFilter};
            let error_context = || ::tideorm::internal::primary_key_error_context::<Self>(
                &id,
                format!("find({})", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&id)),
            );
            let connection = #connection_expr;
            let rows = ::tideorm::profiling::__profile_future(
                ::tideorm::internal::scoped_find::<Self>()
                    .filter(<Self as ::tideorm::internal::InternalModel>::primary_key_condition(&id))
                    .all(&connection.executor()),
            )
                .await
                .map_err(::tideorm::Error::from)
                .map_err(|err| err.with_context(error_context()))?;
            // The baseline dirty tracking keeps is the one of the database
            // the row came from, `db` for `find_with`.
            ::tideorm::model::__loading_through(&connection, || {
                rows.into_iter()
                    .next()
                    .map(<Self as ::tideorm::internal::InternalModel>::try_from_entity_model)
                    .transpose()
            })
        }
    };
    let ensure_fields_storable = ctx.ensure_fields_storable();
    // Insert `model` as a new row and bind `model` to the row as stored, with
    // no callback: `create()` runs its own around it, and an upsert of a key
    // the database has not numbered runs none, as every upsert.
    let insert_new_row = quote! {
        let error_context = || ::tideorm::internal::model_error_context::<Self>(
            format!("insert into {}", #table_name),
        );
        let active = <Self as ::tideorm::internal::InternalModel>::try_into_active_model(model)?;
        let connection = ::tideorm::database::__current_connection()?;
        #ensure_fields_storable
        let result = ::tideorm::profiling::__profile_future(
            ::tideorm::orm::ActiveModelTrait::insert(active, &connection.executor()),
        )
            .await
            .map_err(::tideorm::Error::from)
            .map_err(|err| err.with_context(error_context()))?;
        let model = <Self as ::tideorm::internal::InternalModel>::try_from_entity_model(result)?;
        ::tideorm::QueryCache::global().invalidate_model(#table_name);
    };
    let find_impl = find_body(quote! { ::tideorm::database::__current_connection()? });
    let find_with_impl = find_body(quote! { db.__get_connection()? });

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::model::Model for #struct_name {
            fn primary_key(&self) -> Self::PrimaryKey {
                #primary_key_impl
            }

            async fn find(id: Self::PrimaryKey) -> ::tideorm::Result<Option<Self>> {
                #find_impl
            }

            async fn find_with(
                id: Self::PrimaryKey,
                db: &::tideorm::database::Database,
            ) -> ::tideorm::Result<Option<Self>> {
                #find_with_impl
            }

            async fn destroy(id: Self::PrimaryKey) -> ::tideorm::Result<u64> {
                // Load the row first so `destroy(id)` runs the same delete, and the
                // same callbacks, as `delete(self)`; a trashed row is not found.
                match <Self as ::tideorm::model::Model>::find(id).await? {
                    Some(model) => <Self as ::tideorm::model::Model>::delete(model).await,
                    None => Ok(0),
                }
            }

            async fn create(model: Self) -> ::tideorm::Result<Self> {
                use ::tideorm::callbacks::{
                    AfterCreateDispatch, AfterValidationDispatch, BeforeCreateOnlyDispatch,
                    BeforeSaveDispatch, BeforeValidationDispatch,
                };
                let mut model = model;
                (&mut model).run_before_validation()?;
                ::tideorm::validation::Validate::validate(&model)
                    .map_err(::tideorm::Error::from)?;
                (&model).run_after_validation()?;
                (&mut model).run_before_save()?;
                (&mut model).run_before_create_only()?;
                #insert_new_row
                (&model).run_after_create()?;
                Ok(model)
            }

            async fn delete(self) -> ::tideorm::Result<u64> {
                use ::tideorm::callbacks::{AfterDeleteDispatch, BeforeDeleteDispatch};
                if !<Self as ::tideorm::model::ModelMeta>::soft_delete_enabled() {
                    return <Self as ::tideorm::model::Model>::__force_delete(self).await;
                }
                // A soft-delete model is marked, not removed.
                let model = self;
                (&model).run_before_delete()?;
                let rows =
                    ::tideorm::model::__soft_delete_by_primary_key::<Self>(&model.primary_key()).await?;
                if rows > 0 {
                    ::tideorm::model::__forget_dirty_snapshot(&model);
                }
                (&model).run_after_delete()?;
                Ok(rows)
            }

            async fn __force_delete(self) -> ::tideorm::Result<u64> {
                use ::tideorm::orm::{EntityTrait, QueryFilter};
                use ::tideorm::callbacks::{AfterDeleteDispatch, BeforeDeleteDispatch};
                let model = self;
                (&model).run_before_delete()?;
                let primary_key = model.primary_key();
                let error_context = || ::tideorm::internal::primary_key_error_context::<Self>(
                    &primary_key,
                    format!("delete where {}", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&primary_key)),
                );
                let connection = ::tideorm::database::__current_connection()?;
                // The key condition binds a `u64` past `i64::MAX` as a decimal,
                // where the model's own value would panic the driver.
                let result = ::tideorm::profiling::__profile_future(
                    #internal_entity_mod::Entity::delete_many()
                        .filter(<Self as ::tideorm::internal::InternalModel>::primary_key_condition(&primary_key))
                        .exec(&connection.executor()),
                )
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context()))?;
                if result.rows_affected > 0 {
                    ::tideorm::QueryCache::global().invalidate_model(#table_name);
                    ::tideorm::model::__forget_dirty_snapshot(&model);
                }
                (&model).run_after_delete()?;
                Ok(result.rows_affected)
            }

            async fn save(self) -> ::tideorm::Result<Self> {
                if self.is_new() {
                    Self::create(self).await
                } else {
                    let primary_key = self.primary_key();
                    // The row-presence probe counts trashed rows: saving over a
                    // trashed natural-key row must UPDATE it, not INSERT into a
                    // conflict with it.
                    if <Self as ::tideorm::model::ModelMeta>::primary_key_auto_increment()
                        && <Self as ::tideorm::model::ModelMeta>::primary_key_names().len() == 1
                    {
                        self.update().await
                    } else if ::tideorm::model::__exists_including_trashed::<Self>(&primary_key).await? {
                        self.update().await
                    } else {
                        Self::create(self).await
                    }
                }
            }

            async fn update(self) -> ::tideorm::Result<Self> {
                use ::tideorm::callbacks::{
                    AfterUpdateDispatch, AfterValidationDispatch, BeforeSaveDispatch,
                    BeforeUpdateOnlyDispatch, BeforeValidationDispatch,
                };
                use ::tideorm::orm::ActiveModelTrait;
                let mut model = self;
                (&mut model).run_before_validation()?;
                ::tideorm::validation::Validate::validate(&model)
                    .map_err(::tideorm::Error::from)?;
                (&model).run_after_validation()?;
                (&mut model).run_before_save()?;
                (&mut model).run_before_update_only()?;
                let primary_key = model.primary_key();
                let error_context = || ::tideorm::internal::primary_key_error_context::<Self>(
                    &primary_key,
                    format!("update where {}", <Self as ::tideorm::model::ModelMeta>::primary_key_display(&primary_key)),
                );
                let active = model.__into_update_active_model()?;
                let connection = ::tideorm::database::__current_connection()?;
                #ensure_fields_storable
                let result = ::tideorm::profiling::__profile_future(active.update(&connection.executor()))
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context()))?;
                let model = <Self as ::tideorm::internal::InternalModel>::try_from_entity_model(result)?;
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
                use ::tideorm::internal::InternalModel;
                use ::tideorm::orm::{ColumnTrait, EntityTrait, QueryFilter};
                use ::tideorm::orm::sea_query::OnConflict;

                ::tideorm::validation::Validate::validate(&model)
                    .map_err(::tideorm::Error::from)?;
                #key_uuid_keys
                let model_for_lookup = model.clone();
                let conflict_cols = builder.conflict_columns;
                #encrypted_conflict_column_check
                // The column lists take field or column names; they are
                // compared as column names.
                let conflict_cols: Vec<String> = conflict_cols
                    .into_iter()
                    .map(|column| {
                        <Self as ::tideorm::model::ModelMeta>::canonical_column_name(&column)
                            .map_or(column, str::to_string)
                    })
                    .collect();
                // A key the database has not numbered yet conflicts with no
                // row: keyed by it alone the upsert inserts the row, running
                // no callback, as no upsert does, and otherwise the database
                // numbers it as an insert would, rather than storing its
                // placeholder `0`.
                let key_is_new = #pk_auto_increment
                    && <Self as ::tideorm::model::Model>::is_new(&model_for_lookup);
                if key_is_new && conflict_cols.iter().all(|column| #is_pk_column) {
                    #insert_new_row
                    return Ok(model);
                }
                let include_pk = !key_is_new
                    && (conflict_cols.iter().any(|column| #is_pk_column) || !#pk_auto_increment);
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
                // Unless named explicitly, a managed `created_at` keeps the stored
                // row's creation time when the insert turns into an update.
                let managed_created_at: &[&str] = &[#(#managed_created_at_columns),*];
                let update_cols: Vec<String> = if let Some(cols) = builder.update_columns {
                    cols
                } else if let Some(exclude) = builder.exclude_columns {
                    let exclude = exclude
                        .iter()
                        .map(|name| {
                            <Self as ::tideorm::model::ModelMeta>::canonical_column_name(name).ok_or_else(|| {
                                ::tideorm::Error::invalid_query(format!(
                                    "unknown column '{}' in update_all_except() for {}",
                                    name,
                                    #table_name
                                ))
                            })
                        })
                        .collect::<::tideorm::Result<Vec<&str>>>()?;
                    // The key and the conflict columns are what matched the
                    // stored row, so they stay as they are, as without
                    // `update_all_except`.
                    insertable_columns
                        .iter()
                        .filter(|column| {
                            !exclude.contains(*column) && !managed_created_at.contains(column) && {
                                let column = column.to_string();
                                !conflict_cols.contains(&column) && !#is_pk_column
                            }
                        })
                        .map(|column| column.to_string())
                        .collect()
                } else {
                    insertable_columns.iter().filter(|column| {
                        !managed_created_at.contains(column) && {
                            let column = column.to_string();
                            !conflict_cols.contains(&column) && !#is_pk_column
                        }
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

                // Nothing to overwrite: a conflict keeps the stored row. MySQL
                // has no `DO NOTHING` and takes `ON DUPLICATE KEY UPDATE c = c`
                // over the conflict columns instead.
                let on_conflict = if update_columns.is_empty() {
                    OnConflict::columns(conflict_columns.iter().cloned())
                        .do_nothing_on(conflict_columns.iter().cloned())
                        .to_owned()
                } else {
                    OnConflict::columns(conflict_columns.iter().cloned())
                        .update_columns(update_columns.iter().cloned())
                        .to_owned()
                };

                let error_context = || ::tideorm::internal::model_error_context::<Self>(format!(
                    "insert_or_update into {} on conflict ({})",
                    #table_name,
                    conflict_cols.join(", ")
                ));

                // The insert half stamps managed timestamps like `create()` does; a
                // conflict on the key needs the key in the insert as well.
                let mut active_model = <Self as InternalModel>::try_into_active_model(model)?;
                if include_pk && #pk_auto_increment {
                    #(active_model.#pk_idents = ::tideorm::orm::ActiveValue::Set(model_for_lookup.#pk_idents.clone());)*
                }

                let connection = ::tideorm::database::__current_connection()?;
                #ensure_fields_storable
                // A plain insert reports a conflict that wrote nothing as an
                // error: a kept row, and on MySQL an update that changed no
                // value, which reports no affected row. The row is read back
                // below either way.
                ::tideorm::profiling::__profile_future(
                    #internal_entity_mod::Entity::insert(active_model)
                        .on_conflict(on_conflict)
                        .do_nothing()
                        .exec(&connection.executor()),
                )
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context()))?;
                ::tideorm::QueryCache::global().invalidate_model(#table_name);

                let mut finder = #internal_entity_mod::Entity::find();
                for conflict_column in &conflict_cols {
                    finder = match conflict_column.as_str() {
                        #(#name_patterns => finder.filter(#internal_entity_mod::Column::#column_variants.eq(model_for_lookup.#field_idents.clone())),)*
                        _ => {
                            return Err(::tideorm::Error::invalid_query(format!(
                                "unknown conflict column '{}' for {}",
                                conflict_column,
                                #table_name
                            )));
                        }
                    };
                }

                // MySQL accepts conflict columns no unique key covers, so this
                // reads one row, with a literal `LIMIT 1` rather than `one()`'s
                // bound one.
                let row = ::tideorm::internal::first_row(finder, &connection.executor())
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context()))?;

                row.map(<Self as InternalModel>::try_from_entity_model)
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
