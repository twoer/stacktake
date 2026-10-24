import { describe, expect, it } from "vitest";
import type { AssetGroup, Item } from "@/lib/types";
import {
  canCleanArtifactsItem,
  canCleanItem,
  cleanKindOf,
  isBlockerTag,
  orderedTags,
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
