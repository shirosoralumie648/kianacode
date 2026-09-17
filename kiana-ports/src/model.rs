use async_trait::async_trait;
use kiana_domain::*;

#[async_trait]
/// Runner 使用的异步模型客户端端口。
///
/// 实现只负责将消息交给模型并返回声明；它不得直接执行工具或修改工作区。
pub trait ModelClient: Send + Sync {
    /// Compile once; only the frozen request may be admitted and sent.
    fn prepare_call(
        &self,
        request: ModelRequest,
        spec: ModelCallSpec,
    ) -> Result<PreparedModelCall, ModelError> {
        let context = self.request_context(&request);
        let mut prepared = PreparedModelCall {
            schema: MODEL_CALL_SCHEMA.to_owned(),
            spec,
            route: ModelRoute {
                provider_id: "legacy".to_owned(),
                protocol: ModelProtocol::Legacy,
                connection_id: "in_process".to_owned(),
                model_id: context
                    .model_id
                    .clone()
                    .unwrap_or_else(|| "scripted".to_owned()),
                profile: "legacy".to_owned(),
                configuration_revision: "legacy.v1".to_owned(),
                streaming: false,
            },
            wire_body: serde_json::to_value(&request)
                .map_err(|_| ModelError::invalid("model_request_invalid"))?,
            budget: context.budget(),
            tool_catalog_hash: kiana_domain::json_digest(&serde_json::json!(request.tools)),
            request,
            request_hash: String::new(),
        };
        prepared.seal();
        prepared.validate()?;
        Ok(prepared)
    }

    /// Exactly one attempt. Retries belong to the admitted Harness attempt driver.
    async fn complete_prepared(
        &self,
        prepared: PreparedModelCall,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        prepared.validate()?;
        let output = self
            .complete_streaming(prepared.request, on_delta)
            .await
            .map_err(ModelError::invalid)?;
        ModelReply::legacy(output)
    }

    async fn complete_admitted(
        &self,
        prepared: PreparedModelCall,
        permit: ModelCallPermit,
        admission: &dyn crate::ModelBudgetPort,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelReply, ModelError> {
        admission
            .consume_prepared(&prepared, &permit)
            .await
            .map_err(|e| ModelError::invalid(e.to_string()))?;
        self.complete_prepared(prepared, on_delta).await
    }

    /// Describe the exact system prompt and wire accounting before any network call.
    fn request_context(&self, request: &ModelRequest) -> ModelRequestContext {
        ModelRequestContext::for_request(request)
    }

    /// 完成一次模型轮次。
    ///
    /// 返回错误时 Runner 应按 `model_unavailable`/脚本错误处理，不应伪造空的成功结果。
    async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String>;

    /// 完成一次模型轮次，并在文本生成时交付增量。
    ///
    /// 默认实现保持所有现有客户端的对象安全兼容性：先调用 [`ModelClient::complete`]，
    /// 再把完整文本作为一条 [`ModelDelta::Text`] 交给 `on_delta`；文本为空时不调用回调。
    ///
    /// `on_delta` 是同步回调，不创建后台任务。调用方可在回调中转发到自己的 channel，并通过
    /// 返回错误表达背压或取消。原生流式实现可沿用该回调交付工具调用、usage 和
    /// stop_reason 增量，最终仍返回完整聚合的 [`ModelOutput`]。
    async fn complete_streaming(
        &self,
        request: ModelRequest,
        on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
    ) -> Result<ModelOutput, String> {
        let output = self.complete(request).await?;
        if !output.text.is_empty() {
            on_delta(ModelDelta::Text {
                text: output.text.clone(),
            })?;
        }
        Ok(output)
    }
}
