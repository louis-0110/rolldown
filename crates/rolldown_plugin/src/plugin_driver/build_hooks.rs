use std::sync::Arc;

use crate::{
  HookBuildEndArgs, HookLoadArgs, HookLoadReturn, HookNoopReturn, HookResolveIdArgs,
  HookResolveIdReturn, HookTransformArgs, PluginContext, PluginDriver, TransformPluginContext,
  pluginable::HookTransformAstReturn,
  types::{
    hook_resolve_id_skipped::HookResolveIdSkipped, hook_transform_ast_args::HookTransformAstArgs,
    plugin_idx::PluginIdx,
  },
};
use anyhow::Result;
use rolldown_common::{
  ModuleInfo, ModuleType, NormalModule, SharedNormalizedBundlerOptions,
  side_effects::HookSideEffects,
};
use rolldown_sourcemap::SourceMap;
use rolldown_utils::unique_arc::UniqueArc;
use string_wizard::{MagicString, SourceMapOptions};

impl PluginDriver {
  #[tracing::instrument(level = "trace", skip_all)]
  pub async fn build_start(&self, opts: &SharedNormalizedBundlerOptions) -> HookNoopReturn {
    // let ret = {
    //   #[cfg(not(target_arch = "wasm32"))]
    //   {
    //     block_on_spawn_all(
    //       self
    //         .iter_plugin_with_context_by_order(&self.order_by_build_start_meta)
    //         .map(|(_, plugin, ctx)| plugin.call_build_start(ctx)),
    //     )
    //     .await
    //   }
    //   #[cfg(target_arch = "wasm32")]
    //   {
    //     // FIXME(hyf0): This is a workaround for wasm32 target, it's wired that
    //     // `block_on_spawn_all(self.plugins.iter().map(|(plugin, ctx)| plugin.build_start(ctx))).await;` will emit compile errors like
    //     // `implementation of `std::marker::Send` is not general enough`. It seems to be the problem related to HRTB, async and iterator.
    //     // I guess we need some rust experts here.
    //     let mut futures = vec![];
    //     for (_, plugin, ctx) in
    //       self.iter_plugin_with_context_by_order(&self.order_by_build_start_meta)
    //     {
    //       futures.push(plugin.call_build_start(ctx));
    //     }
    //     block_on_spawn_all(futures.into_iter()).await
    //   }
    // };

    // for r in ret {
    //   r?;
    // }

    for (_, plugin, ctx) in self.iter_plugin_with_context_by_order(&self.order_by_build_start_meta)
    {
      plugin.call_build_start(ctx, &crate::HookBuildStartArgs { options: opts }).await?;
    }

    Ok(())
  }

  #[inline]
  fn get_resolve_call_skipped_plugins(
    specifier: &str,
    importer: Option<&str>,
    skipped_resolve_calls: Option<&Vec<Arc<HookResolveIdSkipped>>>,
  ) -> Vec<PluginIdx> {
    let mut skipped_plugins = vec![];
    if let Some(skipped_resolve_calls) = skipped_resolve_calls {
      for skip_resolve_call in skipped_resolve_calls {
        if skip_resolve_call.specifier == specifier
          && skip_resolve_call.importer.as_deref() == importer
        {
          skipped_plugins.push(skip_resolve_call.plugin_idx);
        }
      }
    }
    skipped_plugins
  }

  pub async fn resolve_id(
    &self,
    args: &HookResolveIdArgs<'_>,
    skipped_resolve_calls: Option<&Vec<Arc<HookResolveIdSkipped>>>,
  ) -> HookResolveIdReturn {
    let skipped_plugins =
      Self::get_resolve_call_skipped_plugins(args.specifier, args.importer, skipped_resolve_calls);
    for (plugin_idx, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_resolve_id_meta)
    {
      if skipped_plugins.iter().any(|p| *p == plugin_idx) {
        continue;
      }
      if let Some(r) = plugin
        .call_resolve_id(
          &skipped_resolve_calls.map_or_else(
            || ctx.clone(),
            |skipped_resolve_calls| {
              PluginContext::new_shared_with_skipped_resolve_calls(
                ctx,
                skipped_resolve_calls.clone(),
              )
            },
          ),
          args,
        )
        .await?
      {
        return Ok(Some(r));
      }
    }
    Ok(None)
  }

  #[allow(deprecated)]
  // Only for rollup compatibility
  pub async fn resolve_dynamic_import(
    &self,
    args: &HookResolveIdArgs<'_>,
    skipped_resolve_calls: Option<&Vec<Arc<HookResolveIdSkipped>>>,
  ) -> HookResolveIdReturn {
    let skipped_plugins =
      Self::get_resolve_call_skipped_plugins(args.specifier, args.importer, skipped_resolve_calls);
    for (plugin_idx, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_resolve_dynamic_import_meta)
    {
      if skipped_plugins.iter().any(|p| *p == plugin_idx) {
        continue;
      }
      if let Some(r) = plugin
        .call_resolve_dynamic_import(
          &skipped_resolve_calls.map_or_else(
            || ctx.clone(),
            |skipped_resolve_calls| {
              PluginContext::new_shared_with_skipped_resolve_calls(
                ctx,
                skipped_resolve_calls.clone(),
              )
            },
          ),
          args,
        )
        .await?
      {
        return Ok(Some(r));
      }
    }
    Ok(None)
  }

  pub async fn load(&self, args: &HookLoadArgs<'_>) -> HookLoadReturn {
    for (_plugin_idx, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_load_meta)
    {
      if let Some(r) = plugin.call_load(ctx, args).await? {
        return Ok(Some(r));
      }
    }
    Ok(None)
  }

  pub async fn transform(
    &self,
    id: &str,
    original_code: String,
    sourcemap_chain: &mut Vec<SourceMap>,
    side_effects: &mut Option<HookSideEffects>,
    module_type: &mut ModuleType,
  ) -> Result<String> {
    let mut code = original_code;
    let mut original_sourcemap_chain = std::mem::take(sourcemap_chain);
    let mut plugin_sourcemap_chain = UniqueArc::new(original_sourcemap_chain);
    for (_plugin_idx, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_transform_meta)
    {
      if let Some(r) = plugin
        .call_transform(
          Arc::new(TransformPluginContext::new(
            ctx.clone(),
            plugin_sourcemap_chain.weak_ref(),
            code.as_str().into(),
            id.into(),
          )),
          &HookTransformArgs { id, code: &code, module_type: &*module_type },
        )
        .await?
      {
        original_sourcemap_chain = plugin_sourcemap_chain.into_inner();
        if let Some(map) = Self::normalize_transform_sourcemap(r.map, id, &code, r.code.as_ref()) {
          original_sourcemap_chain.push(map);
        }
        plugin_sourcemap_chain = UniqueArc::new(original_sourcemap_chain);
        if let Some(v) = r.side_effects {
          *side_effects = Some(v);
        }
        if let Some(v) = r.code {
          code = v;
        }
        if let Some(ty) = r.module_type {
          *module_type = ty;
        }
      }
    }
    *sourcemap_chain = plugin_sourcemap_chain.into_inner();
    Ok(code)
  }

  #[inline]
  fn normalize_transform_sourcemap(
    map: Option<SourceMap>,
    id: &str,
    original_code: &str,
    code: Option<&String>,
  ) -> Option<SourceMap> {
    if let Some(mut map) = map {
      // If sourcemap  hasn't `sources`, using original id to fill it.
      let source = map.get_source(0);
      if source.is_none_or(str::is_empty)
        || (map.get_sources().count() == 1 && (source != Some(id)))
      {
        map.set_sources(vec![id]);
      }
      // If sourcemap hasn't `sourcesContent`, using original code to fill it.
      if map.get_source_content(0).is_none_or(str::is_empty) {
        map.set_source_contents(vec![original_code]);
      }
      Some(map)
    } else if let Some(code) = code {
      if original_code == code {
        None
      } else {
        // If sourcemap is empty and code has changed, need to create one remapping original code.
        // Here using `hires: true` to get more accurate column information, but it has more overhead.
        // TODO: maybe it should be add a option to control hires.
        let magic_string = MagicString::new(original_code);
        Some(magic_string.source_map(SourceMapOptions {
          hires: string_wizard::Hires::True,
          include_content: true,
          source: id.into(),
        }))
      }
    } else {
      None
    }
  }

  pub async fn transform_ast(&self, mut args: HookTransformAstArgs<'_>) -> HookTransformAstReturn {
    for (_, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_transform_ast_meta)
    {
      args.ast = plugin
        .call_transform_ast(
          ctx,
          HookTransformAstArgs {
            cwd: args.cwd,
            ast: args.ast,
            id: args.id,
            is_user_defined_entry: args.is_user_defined_entry,
          },
        )
        .await?;
    }
    Ok(args.ast)
  }

  pub async fn module_parsed(
    &self,
    module_info: Arc<ModuleInfo>,
    normal_module: &NormalModule,
  ) -> HookNoopReturn {
    for (_, plugin, ctx) in
      self.iter_plugin_with_context_by_order(&self.order_by_module_parsed_meta)
    {
      plugin.call_module_parsed(ctx, Arc::clone(&module_info), normal_module).await?;
    }
    Ok(())
  }

  pub async fn build_end(&self, args: Option<&HookBuildEndArgs<'_>>) -> HookNoopReturn {
    for (_, plugin, ctx) in self.iter_plugin_with_context_by_order(&self.order_by_build_end_meta) {
      plugin.call_build_end(ctx, args).await?;
    }
    Ok(())
  }
}
