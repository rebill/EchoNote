import { describe, expect, it } from "vitest";

import { diff, tokens } from "./diff";
import { inline, parse, plainText, timeMs, withoutTitle, type Block, type Inline } from "./markdown";

const text = (nodes: Inline[]) => plainText(nodes);

describe("markdown", () => {
  it("parses the blocks notes use", () => {
    const blocks = parse(
      [
        "## 概要",
        "讨论了**发布计划**。",
        "第二行",
        "",
        "- 第一条 [01:05]",
        "  - 子条目",
        "- [x] 完成的待办",
        "",
        "1. 一",
        "2. 二",
        "",
        "> 引用",
        "",
        "---",
        "```",
        "<b>code</b>",
        "```",
        "| 人 | 事 |",
        "|---|:-:|",
        "| 张三 | 写文档 |",
      ].join("\n"),
    );
    expect(blocks.map((block) => block.kind)).toEqual([
      "heading",
      "paragraph",
      "list",
      "list",
      "quote",
      "rule",
      "code",
      "table",
    ]);
    const [heading, paragraph, bullets, numbers, , , code, table] = blocks as [
      Extract<Block, { kind: "heading" }>,
      Extract<Block, { kind: "paragraph" }>,
      Extract<Block, { kind: "list" }>,
      Extract<Block, { kind: "list" }>,
      Block,
      Block,
      Extract<Block, { kind: "code" }>,
      Extract<Block, { kind: "table" }>,
    ];
    expect(heading.level).toBe(2);
    expect(paragraph.inline[1]).toEqual({ kind: "strong", children: [{ kind: "text", text: "发布计划" }] });
    expect(text(paragraph.inline)).toBe("讨论了发布计划。\n第二行");
    expect(bullets.items).toHaveLength(2);
    expect(bullets.items[0]!.inline[1]).toEqual({ kind: "time", ms: 65_000, label: "01:05" });
    expect(bullets.items[0]!.children[0]!.kind).toBe("list");
    expect(bullets.items[1]!.checked).toBe(true);
    expect(text(bullets.items[1]!.inline)).toBe("完成的待办");
    expect(numbers.ordered).toBe(true);
    expect(code.text).toBe("<b>code</b>");
    expect(table.head.map(text)).toEqual(["人", "事"]);
    expect(table.rows[0]!.map(text)).toEqual(["张三", "写文档"]);
  });

  it("never turns links, images or HTML into anything but text", () => {
    const nodes = inline('看 [这里](https://evil.example/x?q=1) 和 ![图](https://evil.example/p.png) <img src=x onerror=1>');
    expect(nodes).toEqual([{ kind: "text", text: "看 这里 和 图 <img src=x onerror=1>" }]);
    expect(text(inline("a\\*b\\* \\[1:00\\]"))).toBe("a*b* [1:00]");
  });

  it("reads emphasis but not snake_case or lone stars", () => {
    expect(inline("snake_case_name")).toEqual([{ kind: "text", text: "snake_case_name" }]);
    expect(inline("2 * 3 * 4")).toEqual([{ kind: "text", text: "2 * 3 * 4" }]);
    expect(inline("*a* and _b_")[0]).toEqual({ kind: "em", children: [{ kind: "text", text: "a" }] });
    expect(inline("`x * y`")).toEqual([{ kind: "code", text: "x * y" }]);
  });

  it("finds time citations", () => {
    expect(timeMs("12:34")).toBe(754_000);
    expect(timeMs("1:02:03")).toBe(3_723_000);
    expect(timeMs("12:75")).toBeNull();
    const nodes = inline("见 [1:00, 2:30] 和 [12:99] 与 [abc]");
    expect(nodes.filter((node) => node.kind === "time").map((node) => (node as { ms: number }).ms)).toEqual([
      60_000, 150_000,
    ]);
    expect(text(nodes)).toBe("见 [[1:00], [2:30]] 和 [12:99] 与 [abc]");
  });

  it("drops the title line while notes stream", () => {
    expect(withoutTitle("# 周会\n\n## 概要\n…")).toBe("\n## 概要\n…");
    expect(withoutTitle("# 周")).toBe("");
    expect(withoutTitle("## 概要")).toBe("## 概要");
  });

  it("does not hang or explode on hostile input", () => {
    const deep = "> ".repeat(50) + "x";
    expect(() => parse(deep)).not.toThrow();
    const stars = "*".repeat(5000);
    expect(text(inline(stars))).toBe(stars);
    expect(parse("- a\n\n  - b\n\n- c")[0]).toMatchObject({ kind: "list" });
  });
});

describe("diff", () => {
  it("diffs Chinese by character and English by word", () => {
    expect(diff("打开非书看一下", "打开飞书看一下")).toEqual([
      { kind: "same", text: "打开" },
      { kind: "del", text: "非" },
      { kind: "add", text: "飞" },
      { kind: "same", text: "书看一下" },
    ]);
    expect(diff("we use cubernetes now", "we use Kubernetes now")).toEqual([
      { kind: "same", text: "we use " },
      { kind: "del", text: "cubernetes" },
      { kind: "add", text: "Kubernetes" },
      { kind: "same", text: " now" },
    ]);
    expect(tokens("Hi 你好")).toEqual(["Hi", " ", "你", "好"]);
    expect(diff("same", "same")).toEqual([{ kind: "same", text: "same" }]);
  });

  it("replaces very long lines whole", () => {
    const before = "字".repeat(1000);
    const after = "词".repeat(1000);
    expect(diff(before, after)).toEqual([
      { kind: "del", text: before },
      { kind: "add", text: after },
    ]);
  });
});
