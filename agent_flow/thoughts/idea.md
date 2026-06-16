# 🚀 TinyFusion MVP (Minimum Viable Product) 需求规格说明书

## 1. 产品定位与技术愿景
`TinyFusion` 是一款基于 Tauri (Rust) 开发的、本地运行的 **AI API 智能编排网关桌面客户端**。

* **核心功能：** 通过“时序特征嗅探”拦截开发工具（如 OpenCode `/autoplan`）的请求流，在【诊断期】触发多模型并发与结构化裁判机制提升智商，在【执行期】将工具调用直连透传给本地高速 Flash 模型。
* **技术特色：** 极致轻量（Tiny），常驻后台内存占用 <30MB；对上伪装成标准的 OpenAI 协议端点，对下实现“思考与执行分离”的认知层路由。

---

## 2. 核心架构与 OpenRouter 机制本地映射

为了让本地 Agent 完美理解并实现 `TinyFusion` 的核心算法，底层必须精准实现以下三项逻辑：

### 2.1 时序状态嗅探 (Sequential State Sniffing)
网关在单次长对话（Session）内，无需修改 OpenCode 源码，通过请求时序和 Payload 特征动态分流：
1.  **阶段一（高认知诊断）：** 对话的第 1 轮请求。特征为 Context 包含大量报错日志（StackTrace），或 Prompt 结尾为 `Analyze this issue`。➡️ **激活 Fusion 阵列。**
2.  **阶段二（机械式执行）：** 对话的第 2 轮及后续请求。特征为 Context 已包含阶段一的诊断历史，且当前 Prompt 出现明确的工具调用声明（如 `<tool_call>` 或 `Apply changes to file.ts`）。➡️ **关闭 Fusion，直连透传。**

### 2.2 泛出作答与上下文增强 (Fan-Out & Context Injection)
在【诊断期】，网关执行异步并发（Rust `tokio::spawn`）呼叫用户配置的 2~3 个中等模型（如本地 Ollama 部署的 Qwen-2.5-Coder）。分发时，网关在 Prompt 尾部自动注入本地抓取到的增强上下文（如最新的 Linter 错误、关联文件的代码片段）。

### 2.3 裁判的五大维度结构化审计 (Structured Judge)
网关拼接所有 Worker 的文本回复，组装成全新 Payload 发给高智商裁判模型（如云端 Claude），通过 System Prompt 强约束其必须输出以下五种结构化标签：
* `<consensus>`：专家模型一致认定需要修改的代码位置。
* `<contradictions>`：模型间在修复逻辑上的严重冲突。
* `<coverage_gaps>`：是否漏掉了用户报错日志中的关键细节。
* `<unique_insights>`：某个专家模型独立发现的隐蔽根因（Root Cause）。
* `<final_plan>`：最终拍板、无硬伤的**《最终修复步骤指南》**。

---

## 3. MVP 三步走落地实施路线

### 📅 第一步：简易版多代理客户端（基础跑通）
* **核心目标：** 完成基于 Tauri 的客户端基础架设，能够配置用户现有的多个 API 资产，并暴露出标准的本地代理端点（暂不实现 Fusion 功能）。
* **功能需求：**
    * **Tauri 前端：** 提供一个极简表格，录入模型资产（Ollama 本地地址、云端 API Key 和 Model Name）。
    * **Rust 后端：** 利用 `tokio` 和 `axum` 在本地启动 HTTP 代理服务器（默认端口 `9999`），伪装出 `/v1/chat/completions` 标准接口。
    * **单模型中转验证：** 网关只做最简单的 Passthrough（透传）。用户在 UI 上勾选哪个模型为主力模型，所有请求原样转发并返回 Stream 流，验证链路 100% 畅通。

### 📅 第二步：流式防超时控制与心跳伪装（工程调优）
* **核心目标：** 攻克多模型融合时由于后台计算时间过长（3~5秒）导致前端 CLI 判定超时断开（Timeout）的工程死穴。
* **功能需求：**
    * **流式伪装 (Streaming Spoofing)：** 网关在收到请求后的 500ms 内必须做出响应。
    * **心跳维持机制：** 在后台 Workers 和 Judge 正在计算的空档期，网关持续向前端 CLI 吐出空字符（` `）或符合开发工具规范的思考标签（如 `<thinking>`），用于维持住 HTTP 长连接。一旦后台真实数据产出，立刻切换为最高速冲刷（Stream Flush）真实文本。

### 📅 第三步：修 Bug 场景的 Fusion 机制注入（核心升华）
* **核心目标：** 全面注入 OpenRouter 的 Fusion 机制，专项优化 `/autoplan` 等修 Bug 场景。
* **功能需求：**
    * **三轨模型角色指派：** UI 界面允许用户将资产配置给三个角色：*Workers (中等专家组)*、*Judge (结构化裁判)*、*Executor (机械打字员，推荐本地 DeepSeek-Flash)*。
    * **状态机路由切换：** 激活“两阶段时序状态嗅探”。第一轮请求触发 Fusion 机制，第二轮请求直连 Executor。
    * **状态看板：** 前端界面提供实时状态看板，高亮打印：`[诊断中 - Fusion 激活]` 或 `[执行中 - Flash 透传]`。