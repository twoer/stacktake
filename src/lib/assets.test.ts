import { describe, expect, it } from "vitest";
import type { AssetGroup, Item } from "@/lib/types";
import {
  batchCleanRows,
  batchReclaimOf,
  canCleanArtifactsItem,
  canCleanItem,
  cleanKindOf,
  isBlockerTag,
  orderedTags,
  pruneSelection,
  supportsBatchClean,
  tagHint,
} from "@/lib/assets";

// 这层判据是「界面给不给按钮」的唯一真相（S2 起）；错判的两个方向都有后果：
// 给了按钮服务端拒（用户以为工具坏了），没给按钮（用户以为没有清理功能）。
// vue-tsc 验不了判据逻辑本身，这里补的是 D16 登记时点名的那个缺口。

const item = (over: Partial<Item> = {}): Item => ({
  id: "/tmp/repo-a",
  tags: [],
  name: "repo-a",
  path: "/tmp/repo-a",
  metrics: {},
  ...over,
});

const group = (over: Partial<AssetGroup> = {}): AssetGroup => ({
  id: "repos",
  label: "Git 仓库",
  summary: "",
  fields: [],
  items: [],
  ...over,
});

describe("canCleanItem", () => {
  const cleanable = item({ cleanable: true });

  it("三个条件齐了才给按钮", () => {
    expect(canCleanItem(group({ supports_clean: true }), cleanable)).toBe(true);
  });

  it("组没接删除实现就不给（supports_clean 缺省视作 false）", () => {
    expect(canCleanItem(group(), cleanable)).toBe(false);
  });

  it("没有分组上下文（null）不给", () => {
    expect(canCleanItem(null, cleanable)).toBe(false);
  });

  it("行不可清理不给——toolchain 有官方命令的那类", () => {
    expect(canCleanItem(group({ supports_clean: true }), item())).toBe(false);
  });

  it("非宿主机路径不给——docker / 总览行守卫", () => {
    const virtual = item({ cleanable: true, path: "docker://postgres:16" });
    expect(canCleanItem(group({ supports_clean: true }), virtual)).toBe(false);
  });
});

describe("批量清理范围与选择", () => {
  const toolchain = group({ id: "toolchain", supports_clean: true });
  const cleanable = item({ id: "toolchain:/a", path: "/tmp/a", cleanable: true, reclaimable: 1024 });
  const officialCmd = item({ id: "toolchain:/b", path: "/tmp/b", cleanable: false, reclaimable: 2048 });

  it("只开 toolchain / ai_tools；repos 即使接了删除也不给多选", () => {
    expect(supportsBatchClean(toolchain)).toBe(true);
    expect(supportsBatchClean(group({ id: "ai_tools", supports_clean: true }))).toBe(true);
    expect(supportsBatchClean(group({ id: "repos", supports_clean: true }))).toBe(false);
    expect(supportsBatchClean(group({ id: "toolchain" }))).toBe(false);
    expect(supportsBatchClean(null)).toBe(false);
  });

  it("可勾选行 = 单条清理判据，官方命令行与虚拟路径都排除", () => {
    const virtual = item({ id: "x", path: "docker://x", cleanable: true });
    const rows = batchCleanRows(toolchain, [cleanable, officialCmd, virtual]);
    expect(rows.map((it) => it.id)).toEqual(["toolchain:/a"]);
  });

  it("repos 不返回任何可勾选行（范围关着）", () => {
    const repos = group({ id: "repos", supports_clean: true });
    expect(batchCleanRows(repos, [{ ...cleanable, id: "/tmp/repo-a" }])).toEqual([]);
  });

  it("合计只算选中且仍可清理的条目", () => {
    const selected = new Set(["toolchain:/a", "toolchain:/b", "toolchain:/gone"]);
    expect(batchReclaimOf(toolchain, [cleanable, officialCmd], selected)).toBe(1024);
  });

  it("裁剪丢掉被筛掉/已消失的 id；集合没变时返回原引用", () => {
    const all = new Set(["a", "b"]);
    const pruned = pruneSelection(all, new Set(["a"]));
    expect([...pruned]).toEqual(["a"]);
    expect(pruneSelection(all, new Set(["a", "b"]))).toBe(all);
    expect(pruneSelection(all, new Set())).toEqual(new Set());
  });
});

describe("canCleanArtifactsItem", () => {
  const withArtifacts = item({
    cleanable: false,
    metrics: { artifacts: { t: "b", v: 1024 } },
    artifact_entries: [{ relative_path: "target", size_bytes: 1024 }],
  });

  it("有产物条目且组接了删除才给", () => {
    expect(canCleanArtifactsItem(group({ supports_clean: true }), withArtifacts)).toBe(true);
  });

  it("组没接删除实现不给", () => {
    expect(canCleanArtifactsItem(group(), withArtifacts)).toBe(false);
  });

  it("没有产物条目不给（与 artifacts 指标两个口径都要在）", () => {
    expect(
      canCleanArtifactsItem(group({ supports_clean: true }), item({ metrics: { artifacts: { t: "b", v: 1024 } } })),
    ).toBe(false);
  });

  it("产物体积为 0 不给——老缓存没有该指标随它一起消失", () => {
    const zero = item({ metrics: { artifacts: { t: "b", v: 0 } }, artifact_entries: [{ relative_path: "target", size_bytes: 0 }] });
    expect(canCleanArtifactsItem(group({ supports_clean: true }), zero)).toBe(false);
  });
});

describe("cleanKindOf", () => {
  // 性质分岔只管文案语气（删仓库丢源码 / 删缓存花重下时间 / 删会话丢历史），
  // 能力判据在 Rust 侧——新清理入口不登记会落「缓存」措辞，用例钉住已知三组。
  it("登记过的组各归各的性质", () => {
    expect(cleanKindOf("repos")).toBe("repo");
    expect(cleanKindOf("ai_tools")).toBe("ai");
  });

  it("未登记的组与 null 落「缓存」措辞", () => {
    expect(cleanKindOf("toolchain")).toBe("cache");
    expect(cleanKindOf(null)).toBe("cache");
  });
});

describe("orderedTags / isBlockerTag / tagHint", () => {
  it("阻断项排最前——标签段限宽被裁掉的必须是说明项，不能是预警", () => {
    expect(orderedTags(["陈旧", "未推送", "大"])).toEqual(["未推送", "陈旧", "大"]);
    expect(orderedTags(["有改动", "有 stash", "可清理"])).toEqual(["有改动", "有 stash", "可清理"]);
    expect(orderedTags([])).toEqual([]);
  });

  it("阻断名单与 fail-closed 三态对齐", () => {
    expect(isBlockerTag("未推送")).toBe(true);
    expect(isBlockerTag("有改动")).toBe(true);
    expect(isBlockerTag("有 stash")).toBe(true);
    expect(isBlockerTag("大")).toBe(false);
    expect(isBlockerTag("嵌套")).toBe(false);
  });

  it("认不出的标签不给提示（Hint 没文案就不挂实例）", () => {
    expect(tagHint("大")).toMatch(/200 MB/);
    expect(tagHint("自定义")).toBeUndefined();
  });
});
