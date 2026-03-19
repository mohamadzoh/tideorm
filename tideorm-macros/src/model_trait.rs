use proc_macro2::TokenStream as TokenStream2;
use quote::quote;

use crate::context::BuildContext;
use crate::parse::relation_generic_types;
use crate::relation_gen::generate_with_relations_method;

pub(crate) fn generate_model_support(ctx: &BuildContext) -> TokenStream2 {
    let internal_model_impl = generate_internal_model_impl(ctx);
    let helper_methods_impl = generate_helper_methods_impl(ctx);
    let model_trait_impl = generate_model_trait_impl(ctx);
    let soft_delete_impl = generate_soft_delete_impl(ctx);
    let eager_loader_impl = generate_eager_loader_impl(ctx);
    quote! {
        #internal_model_impl
        #helper_methods_impl
        #model_trait_impl
        #soft_delete_impl
        #eager_loader_impl
    }
}

fn generate_soft_delete_impl(ctx: &BuildContext) -> TokenStream2 {
    if !ctx.soft_delete_enabled {
        return quote! {};
    }

    let struct_name = &ctx.struct_name;
    let deleted_at_ident = ctx
        .soft_delete_field_ident
        .as_ref()
        .expect("soft delete field should be resolved when soft_delete is enabled");
    let deleted_at_column = ctx
        .soft_delete_column_name
        .as_ref()
        .expect("soft delete column should be resolved when soft_delete is enabled");

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::SoftDelete for #struct_name {
            fn deleted_at_column() -> &'static str {
                #deleted_at_column
            }

            fn deleted_at(&self) -> Option<::tideorm::chrono::DateTime<::tideorm::chrono::Utc>> {
                self.#deleted_at_ident.clone()
            }

            fn set_deleted_at(
                &mut self,
                timestamp: Option<::tideorm::chrono::DateTime<::tideorm::chrono::Utc>>,
            ) {
                self.#deleted_at_ident = timestamp;
            }
        }
    }
}

fn generate_internal_model_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let insert_active_model_setters = &ctx.insert_active_model_setters;
    let all_field_names = &ctx.all_field_names;
    let relation_field_defaults = &ctx.relation_field_defaults;
    let pk_column_variant = &ctx.pk_column_variant;
    let field_names = &ctx.field_names;
    let column_names = &ctx.column_names;
    let column_variants = &ctx.column_variants;

    quote! {
        #[doc(hidden)]
        impl ::tideorm::internal::InternalModel for #struct_name {
            type Entity = #internal_entity_mod::Entity;
            type ActiveModel = #internal_entity_mod::ActiveModel;

            fn into_active_model(self) -> Self::ActiveModel {
                use ::tideorm::sea_orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #(#insert_active_model_setters),*
                }
            }

            fn from_sea_model(model: #internal_entity_mod::Model) -> Self {
                Self {
                    #(#all_field_names: model.#all_field_names),*,
                    #(#relation_field_defaults),*
                }
                .with_relations()
            }

            fn to_sea_model(&self) -> <Self::Entity as ::tideorm::sea_orm::EntityTrait>::Model {
                #internal_entity_mod::Model {
                    #(#field_names: self.#field_names.clone()),*
                }
            }

            fn column_from_str(name: &str) -> Option<<Self::Entity as ::tideorm::sea_orm::EntityTrait>::Column> {
                match name {
                    #(#column_names | stringify!(#field_names) => Some(#internal_entity_mod::Column::#column_variants),)*
                    _ => None,
                }
            }

            fn primary_key_column() -> Option<<Self::Entity as ::tideorm::sea_orm::EntityTrait>::Column> {
                Some(#internal_entity_mod::Column::#pk_column_variant)
            }
        }
    }
}

fn generate_helper_methods_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let update_active_model_setters = &ctx.update_active_model_setters;
    let pk_ident = &ctx.pk_ident;
    let table_name = &ctx.table_name;
    let pk_column_name = &ctx.pk_column_name;
    let field_names = &ctx.field_names;
    let column_names = &ctx.column_names;
    let with_relations_method = generate_with_relations_method(ctx);

    quote! {
        impl #struct_name {
            #[doc(hidden)]
            pub(crate) const fn __column_name_eq(left: &str, right: &str) -> bool {
                let left_bytes = left.as_bytes();
                let right_bytes = right.as_bytes();

                if left_bytes.len() != right_bytes.len() {
                    return false;
                }

                let mut index = 0;
                while index < left_bytes.len() {
                    if left_bytes[index] != right_bytes[index] {
                        return false;
                    }
                    index += 1;
                }

                true
            }

            #[doc(hidden)]
            pub(crate) const fn __has_column_name(name: &str) -> bool {
                #(
                    if Self::__column_name_eq(name, #column_names) || Self::__column_name_eq(name, stringify!(#field_names)) {
                        return true;
                    }
                )*

                false
            }

            #[doc(hidden)]
            fn __base_error_context() -> ::tideorm::error::ErrorContext {
                ::tideorm::error::ErrorContext::new().table(#table_name)
            }

            #[doc(hidden)]
            fn __primary_key_error_context(
                primary_key: &<Self as ::tideorm::model::ModelMeta>::PrimaryKey,
            ) -> ::tideorm::error::ErrorContext {
                let condition = format!("{} = {}", #pk_column_name, primary_key);
                Self::__base_error_context()
                    .condition(condition.clone())
                    .operator_chain(condition)
            }

            #[doc(hidden)]
            fn __into_update_active_model(self) -> #internal_entity_mod::ActiveModel {
                use ::tideorm::sea_orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #(#update_active_model_setters),*
                }
            }

            #[doc(hidden)]
            fn __into_delete_active_model(self) -> #internal_entity_mod::ActiveModel {
                use ::tideorm::sea_orm::ActiveValue;
                #internal_entity_mod::ActiveModel {
                    #pk_ident: ActiveValue::Unchanged(self.#pk_ident),
                    ..Default::default()
                }
            }

            #with_relations_method
        }
    }
}

fn generate_model_trait_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let internal_entity_mod = &ctx.internal_entity_mod;
    let pk_ident = &ctx.pk_ident;
    let table_name = &ctx.table_name;
    let pk_column_name = &ctx.pk_column_name;
    let pk_auto_increment = ctx.pk_auto_increment;
    let column_names = &ctx.column_names;
    let column_variants = &ctx.column_variants;
    let field_names = &ctx.field_names;

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::model::Model for #struct_name {
            fn primary_key(&self) -> Self::PrimaryKey {
                self.#pk_ident.clone()
            }

            async fn find(id: Self::PrimaryKey) -> ::tideorm::Result<Option<Self>> {
                use ::tideorm::database::Connection;
                use ::tideorm::internal::InternalModel;
                use ::tideorm::sea_orm::EntityTrait;
                let error_context = Self::__primary_key_error_context(&id)
                    .query(format!("find_by_id({})", id));
                let db = ::tideorm::database::__current_db()?;
                let result = ::tideorm::profiling::__profile_future(async move {
                    match db.__get_connection() {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::find_by_id(id).one(&conn).await
                        }
                        ::tideorm::database::ConnectionRef::Transaction(tx) => {
                            #internal_entity_mod::Entity::find_by_id(id).one(tx.as_ref()).await
                        }
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                Ok(result.map(|model| <Self as InternalModel>::from_sea_model(model)))
            }

            async fn find_with(
                id: Self::PrimaryKey,
                db: &::tideorm::database::Database,
            ) -> ::tideorm::Result<Option<Self>> {
                use ::tideorm::database::Connection;
                use ::tideorm::internal::InternalModel;
                use ::tideorm::sea_orm::EntityTrait;
                let error_context = Self::__primary_key_error_context(&id)
                    .query(format!("find_by_id({})", id));
                let result = ::tideorm::profiling::__profile_future(async move {
                    match db.__get_connection() {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::find_by_id(id).one(&conn).await
                        }
                        ::tideorm::database::ConnectionRef::Transaction(tx) => {
                            #internal_entity_mod::Entity::find_by_id(id).one(tx.as_ref()).await
                        }
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                Ok(result.map(|model| <Self as InternalModel>::from_sea_model(model)))
            }

            async fn destroy(id: Self::PrimaryKey) -> ::tideorm::Result<u64> {
                use ::tideorm::database::Connection;
                use ::tideorm::sea_orm::EntityTrait;
                let error_context = Self::__primary_key_error_context(&id)
                    .query(format!("delete_by_id({})", id));
                let db = ::tideorm::database::__current_db()?;
                let result = ::tideorm::profiling::__profile_future(async move {
                    match db.__get_connection() {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::delete_by_id(id).exec(&conn).await
                        }
                        ::tideorm::database::ConnectionRef::Transaction(tx) => {
                            #internal_entity_mod::Entity::delete_by_id(id).exec(tx.as_ref()).await
                        }
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                Ok(result.rows_affected)
            }

            async fn create(model: Self) -> ::tideorm::Result<Self> { model.save().await }

            async fn delete(self) -> ::tideorm::Result<u64> {
                use ::tideorm::database::Connection;
                use ::tideorm::sea_orm::ActiveModelTrait;
                use ::tideorm::callbacks::{AfterDeleteDispatch, BeforeDeleteDispatch};
                let model = self;
                (&model).run_before_delete()?;
                let error_context = Self::__primary_key_error_context(&model.#pk_ident)
                    .query(format!("delete where {} = {}", #pk_column_name, model.#pk_ident));
                let active = model.clone().__into_delete_active_model();
                let db = ::tideorm::database::__current_db()?;
                let result = ::tideorm::profiling::__profile_future(async move {
                    match db.__get_connection() {
                        ::tideorm::database::ConnectionRef::Database(conn) => active.delete(&conn).await,
                        ::tideorm::database::ConnectionRef::Transaction(tx) => active.delete(tx.as_ref()).await,
                    }
                })
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                (&model).run_after_delete()?;
                Ok(result.rows_affected)
            }

            async fn save(self) -> ::tideorm::Result<Self> {
                use ::tideorm::database::Connection;
                use ::tideorm::callbacks::{AfterCreateDispatch, BeforeCreateDispatch};
                use ::tideorm::internal::InternalModel;
                use ::tideorm::sea_orm::ActiveModelTrait;
                let mut model = self;
                (&mut model).run_before_create()?;
                let error_context = Self::__base_error_context().query(format!("insert into {}", #table_name));
                let active = <Self as InternalModel>::into_active_model(model);
                let db = ::tideorm::database::__current_db()?;
                let result = ::tideorm::profiling::__profile_future(
                    async move {
                        match db.__get_connection() {
                            ::tideorm::database::ConnectionRef::Database(conn) => active.insert(&conn).await,
                            ::tideorm::database::ConnectionRef::Transaction(tx) => active.insert(tx.as_ref()).await,
                        }
                    }
                )
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                let model = <Self as InternalModel>::from_sea_model(result);
                (&model).run_after_create()?;
                Ok(model)
            }

            async fn update(self) -> ::tideorm::Result<Self> {
                use ::tideorm::database::Connection;
                use ::tideorm::callbacks::{AfterUpdateDispatch, BeforeUpdateDispatch};
                use ::tideorm::internal::InternalModel;
                use ::tideorm::sea_orm::ActiveModelTrait;
                let mut model = self;
                (&mut model).run_before_update()?;
                let error_context = Self::__primary_key_error_context(&model.#pk_ident)
                    .query(format!("update where {} = {}", #pk_column_name, model.#pk_ident));
                let active = model.__into_update_active_model();
                let db = ::tideorm::database::__current_db()?;
                let result = ::tideorm::profiling::__profile_future(
                    async move {
                        match db.__get_connection() {
                            ::tideorm::database::ConnectionRef::Database(conn) => active.update(&conn).await,
                            ::tideorm::database::ConnectionRef::Transaction(tx) => active.update(tx.as_ref()).await,
                        }
                    }
                )
                    .await
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;
                let model = <Self as InternalModel>::from_sea_model(result);
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
                use ::tideorm::sea_orm::{ColumnTrait, EntityTrait, IntoActiveModel, QueryFilter};
                use ::tideorm::sea_orm::sea_query::OnConflict;

                let db = ::tideorm::database::__current_db()?;
                let insert_db = db.clone();

                let model_for_lookup = model.clone();
                let conflict_cols = builder.conflict_columns;
                let include_pk = conflict_cols.contains(&#pk_column_name.to_string()) || !#pk_auto_increment;
                let insertable_columns: Vec<&str> = vec![#(#column_names),*]
                    .into_iter()
                    .filter(|column| !(*column == #pk_column_name && #pk_auto_increment && !include_pk))
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
                        !conflict_cols.contains(&column) && column != #pk_column_name
                    }).map(|column| column.to_string()).collect()
                };
                for column in &update_cols {
                    let _ = Self::column_from_str(column).ok_or_else(|| {
                        ::tideorm::Error::invalid_query(format!(
                            "unknown update column '{}' for {}",
                            column,
                            #table_name
                        ))
                    })?;
                }

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

                let sea_model = model.to_sea_model();
                let active_model = if include_pk {
                    sea_model.into_active_model()
                } else {
                    <Self as InternalModel>::into_active_model(model)
                };

                ::tideorm::profiling::__profile_future(async move {
                    match insert_db.__get_connection() {
                        ::tideorm::database::ConnectionRef::Database(conn) => {
                            #internal_entity_mod::Entity::insert(active_model)
                                .on_conflict(on_conflict)
                                .exec(&conn)
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

                let result = match db.__get_connection() {
                    ::tideorm::database::ConnectionRef::Database(conn) => finder.one(&conn).await,
                    ::tideorm::database::ConnectionRef::Transaction(tx) => finder.one(tx.as_ref()).await,
                }
                    .map_err(::tideorm::Error::from)
                    .map_err(|err| err.with_context(error_context))?;

                result
                    .map(<Self as InternalModel>::from_sea_model)
                    .ok_or_else(|| ::tideorm::Error::query("upsert completed but no matching row could be reloaded".to_string()))
            }
        }
    }
}

fn generate_eager_loader_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let relation_arms = build_eager_relation_arms(ctx);

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::relations::EagerLoadModel for #struct_name {
            async fn __eager_load(
                models: &mut [::tideorm::relations::WithRelations<Self>],
                relation_tree: &::tideorm::relations::RelationTree,
            ) -> ::tideorm::Result<()> {
                use ::tideorm::database::Connection;
                use ::tideorm::internal::InternalModel;
                use ::tideorm::sea_orm::{EntityTrait, LoaderTrait};

                if models.is_empty() || relation_tree.is_empty() {
                    return Ok(());
                }

                let sea_models: Vec<_> = models
                    .iter()
                    .map(|entry| entry.model.to_sea_model())
                    .collect();
                let db = ::tideorm::database::__current_db()?;

                for relation_name in relation_tree.roots() {
                    match relation_name.as_str() {
                        #(#relation_arms)*
                        _ => {
                            return Err(::tideorm::Error::query(format!(
                                "Unknown relation '{}' on {}",
                                relation_name,
                                stringify!(#struct_name)
                            )));
                        }
                    }
                }

                Ok(())
            }
        }
    }
}

fn build_eager_relation_arms(ctx: &BuildContext) -> Vec<TokenStream2> {
    ctx.relation_fields
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .filter_map(|(field, ident)| {
            let relation_name = ident.to_string();
            let related_types = relation_generic_types(&field.ty);
            let related_ty = related_types.first()?.clone();

            if field.has_many.is_some() || field.has_many_through.is_some() {
                return Some(quote! {
                    #relation_name => {
                        let nested = relation_tree.get_nested(#relation_name);
                        let loaded = match db.__get_connection() {
                            ::tideorm::database::ConnectionRef::Database(conn) => sea_models
                                .load_many(<<#related_ty as ::tideorm::internal::InternalModel>::Entity as EntityTrait>::find(), &conn)
                                .await,
                            ::tideorm::database::ConnectionRef::Transaction(tx) => sea_models
                                .load_many(<<#related_ty as ::tideorm::internal::InternalModel>::Entity as EntityTrait>::find(), tx.as_ref())
                                .await,
                        }
                            .map_err(|error| ::tideorm::Error::query(error.to_string()))?;

                        for (entry, related_models) in models.iter_mut().zip(loaded.into_iter()) {
                            let mut related_models: Vec<#related_ty> = related_models
                                .into_iter()
                                .map(<#related_ty as ::tideorm::internal::InternalModel>::from_sea_model)
                                .collect();

                            if let Some(nested_tree) = nested {
                                let mut wrapped_related: Vec<::tideorm::relations::WithRelations<#related_ty>> = related_models
                                    .into_iter()
                                    .map(::tideorm::relations::WithRelations::new)
                                    .collect();
                                <#related_ty as ::tideorm::relations::EagerLoadModel>::__eager_load(
                                    &mut wrapped_related,
                                    nested_tree,
                                )
                                .await?;
                                related_models = wrapped_related
                                    .into_iter()
                                    .map(::tideorm::relations::WithRelations::into_inner)
                                    .collect();
                            }

                            entry.model.#ident.set_cached(related_models);
                        }
                    }
                });
            }

            if field.has_one.is_some() || field.belongs_to.is_some() {
                return Some(quote! {
                    #relation_name => {
                        let nested = relation_tree.get_nested(#relation_name);
                        let loaded = match db.__get_connection() {
                            ::tideorm::database::ConnectionRef::Database(conn) => sea_models
                                .load_one(<<#related_ty as ::tideorm::internal::InternalModel>::Entity as EntityTrait>::find(), &conn)
                                .await,
                            ::tideorm::database::ConnectionRef::Transaction(tx) => sea_models
                                .load_one(<<#related_ty as ::tideorm::internal::InternalModel>::Entity as EntityTrait>::find(), tx.as_ref())
                                .await,
                        }
                            .map_err(|error| ::tideorm::Error::query(error.to_string()))?;

                        for (entry, related_model) in models.iter_mut().zip(loaded.into_iter()) {
                            let related_model = match related_model {
                                Some(model) => {
                                    let model = <#related_ty as ::tideorm::internal::InternalModel>::from_sea_model(model);
                                    if let Some(nested_tree) = nested {
                                        let mut wrapped_related = vec![::tideorm::relations::WithRelations::new(model)];
                                        <#related_ty as ::tideorm::relations::EagerLoadModel>::__eager_load(
                                            &mut wrapped_related,
                                            nested_tree,
                                        )
                                        .await?;
                                        wrapped_related
                                            .into_iter()
                                            .next()
                                            .map(::tideorm::relations::WithRelations::into_inner)
                                    } else {
                                        Some(model)
                                    }
                                }
                                None => None,
                            };

                            entry.model.#ident.set_cached(related_model);
                        }
                    }
                });
            }

            None
        })
        .collect()
}
