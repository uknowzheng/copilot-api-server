#!/usr/bin/env python3
"""
交互式 echo 客户端，用来手测本仓库的 OpenAI 兼容服务。

功能：
- 启动时调用 /v1/models 列出真实模型，用户选一个
- 进入 REPL：输入文本 → POST /v1/chat/completions → 打印回复
- 默认流式（SSE），可用 --no-stream 切非流式
- 维护多轮 history（默认开），输入 :reset 清空
- :model 重新选模型；:quit 退出

只依赖 Python 3 标准库。
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request


def http_get_json(url: str, api_key: str | None) -> dict:
    req = urllib.request.Request(url, method="GET")
    if api_key:
        req.add_header("Authorization", f"Bearer {api_key}")
    with urllib.request.urlopen(req, timeout=30) as resp:
        return json.loads(resp.read().decode("utf-8"))


def http_post(url: str, body: dict, api_key: str | None, stream: bool, timeout: float):
    data = json.dumps(body).encode("utf-8")
    req = urllib.request.Request(url, data=data, method="POST")
    req.add_header("Content-Type", "application/json")
    if api_key:
        req.add_header("Authorization", f"Bearer {api_key}")
    return urllib.request.urlopen(req, timeout=timeout)


def list_models(base: str, api_key: str | None) -> list[dict]:
    data = http_get_json(f"{base}/v1/models", api_key)
    items = data.get("data", [])
    if not items:
        raise RuntimeError("/v1/models 返回空")
    return items


def pick_model(models: list[dict], preset: str | None) -> str:
    ids = [m["id"] for m in models]
    if preset:
        if preset not in ids:
            print(f"[warn] 模型 {preset} 不在列表，可选: {ids}", file=sys.stderr)
            sys.exit(1)
        return preset

    print("\n可用模型：")
    for i, m in enumerate(models):
        caps = m.get("capabilities") or {}
        flags = []
        if caps.get("vision"):
            flags.append("vision")
        if caps.get("reasoning_effort"):
            flags.append("reasoning")
        flag_str = f" [{','.join(flags)}]" if flags else ""
        name = m.get("display_name") or m["id"]
        print(f"  {i:>2}. {m['id']:<24} {name}{flag_str}")
    while True:
        raw = input("\n选择模型编号 (默认 0): ").strip()
        if raw == "":
            return ids[0]
        if raw.isdigit() and 0 <= int(raw) < len(ids):
            return ids[int(raw)]
        if raw in ids:
            return raw
        print("无效输入，再试一次")


def chat_blocking(base: str, api_key: str | None, model: str,
                  history: list[dict], user_text: str, timeout: float) -> str:
    messages = history + [{"role": "user", "content": user_text}]
    body = {"model": model, "stream": False, "messages": messages}
    try:
        with http_post(f"{base}/v1/chat/completions", body, api_key, False, timeout) as resp:
            data = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        return f"[HTTP {e.code}] {e.read().decode('utf-8', 'replace')}"

    if "error" in data:
        return f"[error] {data['error']}"
    msg = data["choices"][0]["message"]
    content = msg.get("content") or ""
    if msg.get("tool_calls"):
        content += f"\n[tool_calls] {json.dumps(msg['tool_calls'], ensure_ascii=False)}"
    usage = data.get("usage") or {}
    if usage:
        content += f"\n[usage] prompt={usage.get('prompt_tokens')} completion={usage.get('completion_tokens')}"
    return content


def chat_streaming(base: str, api_key: str | None, model: str,
                   history: list[dict], user_text: str, timeout: float) -> str:
    messages = history + [{"role": "user", "content": user_text}]
    body = {
        "model": model,
        "stream": True,
        "stream_options": {"include_usage": True},
        "messages": messages,
    }
    try:
        resp = http_post(f"{base}/v1/chat/completions", body, api_key, True, timeout)
    except urllib.error.HTTPError as e:
        return f"[HTTP {e.code}] {e.read().decode('utf-8', 'replace')}"

    full = []
    finish_reason = None
    usage = None
    try:
        for raw in resp:
            line = raw.decode("utf-8", "replace").rstrip("\r\n")
            if not line.startswith("data: "):
                continue
            payload = line[6:]
            if payload == "[DONE]":
                break
            try:
                chunk = json.loads(payload)
            except json.JSONDecodeError:
                continue
            if "error" in chunk:
                err = chunk["error"]
                msg = err.get("message") if isinstance(err, dict) else err
                print(f"\n[error] {msg}", file=sys.stderr)
                return ""
            for ch in chunk.get("choices", []):
                delta = ch.get("delta") or {}
                if delta.get("content"):
                    sys.stdout.write(delta["content"])
                    sys.stdout.flush()
                    full.append(delta["content"])
                if delta.get("tool_calls"):
                    sys.stdout.write(f"\n[tool_call delta] {json.dumps(delta['tool_calls'], ensure_ascii=False)}")
                    sys.stdout.flush()
                if ch.get("finish_reason"):
                    finish_reason = ch["finish_reason"]
            if chunk.get("usage"):
                usage = chunk["usage"]
    finally:
        resp.close()

    print()  # newline
    if finish_reason and finish_reason != "stop":
        print(f"[finish_reason={finish_reason}]")
    if usage:
        print(f"[usage] prompt={usage.get('prompt_tokens')} "
              f"completion={usage.get('completion_tokens')} "
              f"total={usage.get('total_tokens')}")
    return "".join(full)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[1])
    ap.add_argument("--base", default=os.environ.get("BASE_URL", "http://127.0.0.1:8080"),
                    help="服务地址 (默认 http://127.0.0.1:8080)")
    ap.add_argument("--api-key", default=os.environ.get("COPILOT_API_KEY"),
                    help="Bearer token (默认读 COPILOT_API_KEY)")
    ap.add_argument("--model", help="预选模型，跳过交互菜单")
    ap.add_argument("--no-stream", action="store_true", help="使用非流式 (默认流式)")
    ap.add_argument("--no-history", action="store_true", help="禁用多轮上下文")
    ap.add_argument("--timeout", type=float, default=120.0, help="单次请求超时秒数")
    ap.add_argument("--once", help="不进入 REPL，发一次该 prompt 就退出")
    args = ap.parse_args()

    base = args.base.rstrip("/")

    print(f"[info] base={base}  stream={'off' if args.no_stream else 'on'}  "
          f"history={'off' if args.no_history else 'on'}")

    try:
        models = list_models(base, args.api_key)
    except Exception as e:
        print(f"[fatal] 拉取 /v1/models 失败: {e}", file=sys.stderr)
        return 1

    model = pick_model(models, args.model)
    print(f"[info] 使用模型: {model}")

    history: list[dict] = []

    def send(text: str) -> None:
        if args.no_stream:
            reply = chat_blocking(base, args.api_key, model, history, text, args.timeout)
            print(reply)
        else:
            reply = chat_streaming(base, args.api_key, model, history, text, args.timeout)
        if not args.no_history and reply:
            history.append({"role": "user", "content": text})
            history.append({"role": "assistant", "content": reply})

    if args.once is not None:
        send(args.once)
        return 0

    print("\n指令: :reset 清空历史   :model 切换模型   :quit 退出")
    while True:
        try:
            text = input("\n> ").strip()
        except (EOFError, KeyboardInterrupt):
            print()
            return 0
        if not text:
            continue
        if text in (":quit", ":q", ":exit"):
            return 0
        if text == ":reset":
            history.clear()
            print("[info] history cleared")
            continue
        if text == ":model":
            model = pick_model(models, None)
            history.clear()
            print(f"[info] 切到 {model}，history 已清空")
            continue
        send(text)


if __name__ == "__main__":
    sys.exit(main())
