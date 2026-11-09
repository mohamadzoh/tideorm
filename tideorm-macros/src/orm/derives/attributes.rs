use quote::ToTokens;
use syn::LitStr;

/// The `#[orm(..)]` arguments of `DeriveValueType`.
#[derive(Default)]
pub(super) struct ValueTypeAttrs {
    pub(super) column_type: Option<LitStr>,
    pub(super) array_type: Option<LitStr>,
    pub(super) value_type: Option<LitStr>,
    pub(super) from_str: Option<LitStr>,
    pub(super) to_str: Option<LitStr>,
    pub(super) try_from_u64: bool,
    pub(super) try_getable_array: bool,
}

impl ValueTypeAttrs {
    /// Reads every `#[orm(..)]` attribute: `None` without one. A repeated
    /// argument keeps its last value.
    pub(super) fn try_from_attributes(attrs: &[syn::Attribute]) -> syn::Result<Option<Self>> {
        let mut found = None;
        for attr in attrs.iter().filter(|attr| attr.path().is_ident("orm")) {
            let args = found.get_or_insert_with(Self::default);
            attr.parse_nested_meta(|meta| {
                let name = meta.path.get_ident().map(ToString::to_string);
                match name.as_deref().unwrap_or_default() {
                    "column_type" => args.column_type = Some(meta.value()?.parse()?),
                    "array_type" => args.array_type = Some(meta.value()?.parse()?),
                    "value_type" => args.value_type = Some(meta.value()?.parse()?),
                    "from_str" => args.from_str = Some(meta.value()?.parse()?),
                    "to_str" => args.to_str = Some(meta.value()?.parse()?),
                    "try_from_u64" => args.try_from_u64 = true,
                    "try_getable_array" => args.try_getable_array = true,
                    _ => {
                        return Err(meta.error(format!(
                            "`#[orm]` got unknown `{}` argument. Supported arguments are \
                             `array_type`, `column_type`, `from_str`, `to_str`, \
                             `try_from_u64`, `try_getable_array`, `value_type`",
                            meta.path.to_token_stream()
                        )));
                    }
                }
                Ok(())
            })?;
        }
        Ok(found)
    }
}
