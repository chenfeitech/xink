# AI HTTP Spike（M0）

验证 `ureq` 对 Ollama 的可行性的**一次性原型**，不属于 xink 的构建产物。
结论与实测数据见 [docs/ai/ollama-provider-design.md](../../docs/ai/ollama-provider-design.md) §2。

运行前提：本地 Ollama 在 `http://localhost:11434` 提供服务（`ollama serve` 或 Ollama.app）。

```bash
cd spike/ai-http
cargo run --release -- qwen3.5:0.8b          # tags + native NDJSON + SSE + 取消
cargo run --release --bin tools -- tev1:4b   # 工具调用两轮闭环
cargo run --release --bin agent --           # Agent + 超时 + 错误分类
```

M1 落地后（`src/core/ai/` 具备等价能力）本目录删除。
