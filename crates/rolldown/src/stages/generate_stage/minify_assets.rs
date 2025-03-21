use oxc::codegen::CodegenOptions;
use rolldown_common::MinifyOptions;
use rolldown_ecmascript::EcmaCompiler;
use rolldown_error::BuildResult;
use rolldown_sourcemap::collapse_sourcemaps;
use rolldown_utils::rayon::{IntoParallelRefMutIterator, ParallelIterator};

use crate::type_alias::IndexAssets;

use super::GenerateStage;

impl GenerateStage<'_> {
  pub fn minify_assets(&self, assets: &mut IndexAssets) -> BuildResult<()> {
    if let MinifyOptions::Enabled(minify_options) = &self.options.minify {
      assets.par_iter_mut().try_for_each(|asset| -> anyhow::Result<()> {
        match asset.meta {
          rolldown_common::InstantiationKind::Ecma(_) => {
            // TODO: Do we need to ensure `asset.filename` to be absolute path?
            let (minified_content, new_map) = EcmaCompiler::minify(
              asset.content.try_as_inner_str()?,
              asset.map.is_some(),
              &asset.filename,
              minify_options.to_oxc_minifier_options(self.options),
              minify_options.compress,
              CodegenOptions { minify: minify_options.remove_whitespace, ..Default::default() },
            );
            asset.content = minified_content.into();
            match (&asset.map, &new_map) {
              (Some(origin_map), Some(new_map)) => {
                asset.map = Some(collapse_sourcemaps(vec![origin_map, new_map]));
              }
              _ => {
                // TODO: Map is dirty. Should we reset the `asset.map` to `None`?
              }
            }
          }
          rolldown_common::InstantiationKind::Css(_) | rolldown_common::InstantiationKind::None => {
          }
        }
        Ok(())
      })?;
    }

    Ok(())
  }
}
