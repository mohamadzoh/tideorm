use super::*;

/// The `Model` impl. The statements run in the runtime (`model::__insert_row`,
/// `__update_row`, `__delete_row`, `__upsert`, `internal::find_by_primary_key`);
/// what stays here is what must: the callback dispatch, which only resolves at
/// a call site where `Self` is the concrete model.
pub(super) fn generate_model_trait_impl(ctx: &BuildContext) -> TokenStream2 {
    let struct_name = &ctx.struct_name;
    let primary_key_impl = build_primary_key_value_impl(ctx);
    // An upsert keys a nil `Uuid` key before it reads the row back by it; the
    // insert conversion would key only the insert.
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

    // A soft-delete model is marked, not removed.
    let delete_body = if ctx.soft_delete.is_some() {
        quote! {
            use ::tideorm::callbacks::{AfterDeleteDispatch, BeforeDeleteDispatch};
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
    } else {
        quote! {
            <Self as ::tideorm::model::Model>::__force_delete(self).await
        }
    };

    quote! {
        #[::tideorm::async_trait::async_trait]
        impl ::tideorm::model::Model for #struct_name {
            fn primary_key(&self) -> Self::PrimaryKey {
                #primary_key_impl
            }

            // The connection is resolved before the lookup, so a missing or
            // unreachable database keeps its `Error::Connection` class.
            async fn find(id: Self::PrimaryKey) -> ::tideorm::Result<Option<Self>> {
                let connection = ::tideorm::database::__current_connection()?;
                ::tideorm::internal::find_by_primary_key::<Self>(&connection, &id, false, "find").await
            }

            async fn find_with(
                id: Self::PrimaryKey,
                db: &::tideorm::database::Database,
            ) -> ::tideorm::Result<Option<Self>> {
                let connection = db.__get_connection()?;
                ::tideorm::internal::find_by_primary_key::<Self>(&connection, &id, false, "find").await
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
                let active = <Self as ::tideorm::internal::InternalModel>::try_into_active_model(model)?;
                let model = ::tideorm::model::__insert_row::<Self>(active).await?;
                (&model).run_after_create()?;
                Ok(model)
            }

            async fn delete(self) -> ::tideorm::Result<u64> {
                #delete_body
            }

            async fn __force_delete(self) -> ::tideorm::Result<u64> {
                use ::tideorm::callbacks::{AfterDeleteDispatch, BeforeDeleteDispatch};
                let model = self;
                (&model).run_before_delete()?;
                let rows = ::tideorm::model::__delete_row(&model).await?;
                (&model).run_after_delete()?;
                Ok(rows)
            }

            async fn update(self) -> ::tideorm::Result<Self> {
                use ::tideorm::callbacks::{
                    AfterUpdateDispatch, AfterValidationDispatch, BeforeSaveDispatch,
                    BeforeUpdateOnlyDispatch, BeforeValidationDispatch,
                };
                let mut model = self;
                (&mut model).run_before_validation()?;
                ::tideorm::validation::Validate::validate(&model)
                    .map_err(::tideorm::Error::from)?;
                (&model).run_after_validation()?;
                (&mut model).run_before_save()?;
                (&mut model).run_before_update_only()?;
                let primary_key = model.primary_key();
                let active = model.__into_update_active_model()?;
                let model = ::tideorm::model::__update_row::<Self>(active, &primary_key).await?;
                (&model).run_after_update()?;
                Ok(model)
            }

            async fn __insert_with_conflict(
                model: Self,
                builder: ::tideorm::model::OnConflictBuilder<Self>,
            ) -> ::tideorm::Result<Self> {
                #key_uuid_keys
                ::tideorm::model::__upsert::<Self>(model, builder, &[#(#managed_created_at_columns),*]).await
            }
        }
    }
}
