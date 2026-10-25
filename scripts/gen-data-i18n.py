#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把 data.* 后端展示文案写进五份语言目录（键序：group → field → tag → kind →
section → item → tool → note → summary → value → error）。"""
import io
import json
import re
import sys
from collections import OrderedDict

# key -> (en, es, ja, zh-CN, zh-TW)
D = OrderedDict()


def k(key, en, es, ja, cn, tw):
    assert key not in D, key
    D[key] = (en, es, ja, cn, tw)


# ---- 分组名（侧栏 / 检查器 / 总览行） ----
k("data.group.repos", "Git repositories", "Repositorios Git", "Git リポジトリ", "Git 仓库", "Git 倉庫")
k("data.group.java", "Java environment", "Entorno Java", "Java 環境", "Java 环境", "Java 環境")
k("data.group.ios", "iOS environment", "Entorno iOS", "iOS 環境", "iOS 环境", "iOS 環境")
k("data.group.android", "Android environment", "Entorno Android", "Android 環境", "Android 环境", "Android 環境")
k("data.group.harmony", "HarmonyOS environment", "Entorno HarmonyOS", "HarmonyOS 環境", "鸿蒙环境", "鴻蒙環境")
k("data.group.toolchain", "Toolchain caches", "Cachés de toolchain", "ツールチェーンのキャッシュ", "工具链缓存", "工具鏈快取")
k("data.group.ai_tools", "Editors & AI tools", "Editores y herramientas de IA", "エディタと AI ツール", "编辑器与 AI 工具", "編輯器與 AI 工具")
k("data.group.local_models", "Local models", "Modelos locales", "ローカルモデル", "本地模型", "本地模型")
k("data.group.wsl", "WSL distros", "Distribuciones de WSL", "WSL ディストリビューション", "WSL 发行版", "WSL 發行版")
k("data.group.docker", "Docker", "Docker", "Docker", "Docker", "Docker")
k("data.group.ssh", "SSH keys", "Claves SSH", "SSH 鍵", "SSH 密钥", "SSH 金鑰")
k("data.group.overview", "Overview", "Resumen", "概要", "总览", "總覽")

# ---- 列头（后端 fields 下发） ----
k("data.field.size", "Size", "Tamaño", "サイズ", "体积", "體積")
k("data.field.lastModified", "Last modified", "Última modificación", "最終更新", "最后修改", "最後修改")
k("data.field.lastCommit", "Last commit", "Último commit", "最終コミット", "最后提交", "最後提交")
k("data.field.branch", "Branch", "Rama", "ブランチ", "分支", "分支")
k("data.field.kind", "Category", "Categoría", "カテゴリ", "类别", "類別")
k("data.field.lastActivity", "Last activity", "Última actividad", "最終アクティビティ", "最后活动", "最後活動")
k("data.field.count", "Items", "Elementos", "件数", "条目", "條目")
k("data.field.reclaimable", "Reclaimable", "Recuperable", "回収可能", "可回收", "可回收")
k("data.field.type", "Type", "Tipo", "種類", "类型", "類型")
k("data.field.created", "Created", "Creado", "作成日時", "创建", "建立")
k("data.field.refs", "References", "Referencias", "参照", "引用", "引用")
k("data.field.usage", "Usage", "Uso", "用途", "用途", "用途")

# ---- 标签 ----
k("data.tag.cleanable", "Cleanable", "Limpiable", "クリーン可能", "可清理", "可清理")
k("data.tag.stale", "Stale", "Obsoleto", "古い", "陈旧", "陳舊")
k("data.tag.large", "Large", "Grande", "大", "大", "大")
k("data.tag.unpushed", "Unpushed", "Sin enviar", "未プッシュ", "未推送", "未推送")
k("data.tag.modified", "Modified", "Con cambios", "変更あり", "有改动", "有改動")
k("data.tag.stashed", "Has stash", "Con stash", "stash あり", "有 stash", "有 stash")
k("data.tag.nested", "Nested", "Anidado", "入れ子", "嵌套", "嵌套")
k("data.tag.artifacts", "Build output", "Con artefactos", "成果物あり", "有产物", "有產物")
k("data.tag.referenced", "Referenced", "Referenciada", "参照済み", "已引用", "已引用")
k("data.tag.unreferenced", "Unreferenced", "Sin referencias", "未参照", "未被引用", "未被引用")
k("data.tag.encrypted", "Encrypted", "Cifrada", "暗号化済み", "加密", "加密")
k("data.tag.noPassphrase", "No passphrase", "Sin passphrase", "パスフレーズなし", "无密码", "無密碼")
k("data.tag.missingPub", "Missing .pub", "Falta .pub", ".pub なし", "缺 .pub", "缺 .pub")
k("data.tag.unused", "Unused", "Sin usar", "未使用", "未使用", "未使用")

# ---- 类别（metrics.kind 的展示名；原值仍是筛选词汇） ----
k("data.kind.downloadCache", "Download cache", "Caché de descargas", "ダウンロードキャッシュ", "下载缓存", "下載快取")
k("data.kind.dependencyCache", "Dependency cache", "Caché de dependencias", "依存関係キャッシュ", "依赖缓存", "相依套件快取")
k("data.kind.buildCache", "Build cache", "Caché de compilación", "ビルドキャッシュ", "构建缓存", "建置快取")
k("data.kind.buildArtifacts", "Build output", "Artefactos de compilación", "ビルド成果物", "构建产物", "建置產物")
k("data.kind.installed", "Installed", "Instalado", "インストール済み", "已安装", "已安裝")
k("data.kind.archive", "Archive", "Archivo", "アーカイブ", "归档", "歸檔")
k("data.kind.deviceSupport", "Device support", "Soporte de dispositivos", "デバイス対応", "设备支持", "裝置支援")
k("data.kind.deviceSymbols", "Device symbols", "Símbolos de dispositivo", "デバイスシンボル", "设备符号", "裝置符號")
k("data.kind.simulator", "Simulator", "Simulador", "シミュレータ", "模拟器", "模擬器")
k("data.kind.simulatorRuntime", "Simulator runtime", "Tiempo de ejecución del simulador", "シミュレータランタイム", "模拟器运行时", "模擬器執行期")
k("data.kind.cache", "Cache", "Caché", "キャッシュ", "缓存", "快取")
k("data.kind.ide", "IDE", "IDE", "IDE", "IDE", "IDE")
k("data.kind.ideData", "IDE data", "Datos del IDE", "IDE データ", "IDE 数据", "IDE 資料")
k("data.kind.systemImage", "System image", "Imagen de sistema", "システムイメージ", "系统镜像", "系統映像")
k("data.kind.sdkComponent", "SDK component", "Componente del SDK", "SDK コンポーネント", "SDK 组件", "SDK 元件")
k("data.kind.config", "Config", "Configuración", "設定", "配置", "設定")
k("data.kind.project", "Project", "Proyecto", "プロジェクト", "工程目录", "工程目錄")
k("data.kind.runtime", "Runtime", "Tiempo de ejecución", "ランタイム", "运行时", "執行期")
k("data.kind.distribution", "Distribution", "Distribución", "ディストリビューション", "分发版", "發行版")
k("data.kind.log", "Logs", "Registros", "ログ", "日志", "日誌")
k("data.kind.session", "Sessions", "Sesiones", "セッション", "会话", "工作階段")
k("data.kind.skill", "Skills", "Habilidades", "スキル", "技能", "技能")
k("data.kind.index", "Index", "Índices", "インデックス", "索引", "索引")
k("data.kind.unknown", "Unknown", "Desconocido", "不明", "未知", "未知")
k("data.kind.virtualDisk", "Virtual disk", "Disco virtual", "仮想ディスク", "虚拟磁盘", "虛擬磁碟")
k("data.kind.wsl1NoDisk", "WSL1 (no virtual disk)", "WSL1 (sin disco virtual)", "WSL1（仮想ディスクなし）", "WSL1（无虚拟磁盘）", "WSL1（無虛擬磁碟）")
k("data.kind.virtualDiskMissing", "Virtual disk missing", "Falta el disco virtual", "仮想ディスクが見つからない", "虚拟磁盘缺失", "虛擬磁碟缺失")

# ---- 分节标题 ----
k("data.section.node", "Node ecosystem", "Ecosistema Node", "Node エコシステム", "Node 生态", "Node 生態系")
k("data.section.aiRuntime", "AI runtime", "Tiempo de ejecución de IA", "AI ランタイム", "AI 运行时", "AI 執行期")
k("data.section.other", "Other", "Otros", "その他", "其它", "其他")
k("data.section.simulators", "Simulators", "Simuladores", "シミュレータ", "模拟器", "模擬器")
k("data.section.mavenRepo", "Maven repository", "Repositorio Maven", "Maven リポジトリ", "Maven 仓库", "Maven 倉庫")

# ---- 行名后半截（toolchain 白名单条目 / dev_env 探测位置） ----
k("data.item.nvmDownloadCache", "nvm download cache", "Caché de descargas de nvm", "nvm ダウンロードキャッシュ", "nvm 下载缓存", "nvm 下載快取")
k("data.item.pnpmStore", "pnpm content store", "Almacén de contenido de pnpm", "pnpm コンテンツストア", "pnpm 内容存储", "pnpm 內容儲存")
k("data.item.pnpmNode", "Node managed by pnpm", "Node gestionado por pnpm", "pnpm が管理する Node", "pnpm 的 Node", "pnpm 的 Node")
k("data.item.nodeGypHeaders", "node-gyp headers", "Cabeceras de node-gyp", "node-gyp ヘッダー", "node-gyp 头文件", "node-gyp 標頭檔")
k("data.item.playwrightBrowsers", "Playwright browsers", "Navegadores de Playwright", "Playwright ブラウザ", "Playwright 浏览器", "Playwright 瀏覽器")
k("data.item.puppeteerBrowsers", "Puppeteer browsers", "Navegadores de Puppeteer", "Puppeteer ブラウザ", "Puppeteer 浏览器", "Puppeteer 瀏覽器")
k("data.item.cypressBinary", "Cypress binaries", "Binarios de Cypress", "Cypress バイナリ", "Cypress 二进制", "Cypress 二進位")
k("data.item.electronXdg", "Electron (XDG)", "Electron (XDG)", "Electron (XDG)", "Electron（XDG）", "Electron（XDG）")
k("data.item.typescriptVersions", "TypeScript versions", "Versiones de TypeScript", "TypeScript バージョン", "TypeScript 版本", "TypeScript 版本")
k("data.item.rustupToolchains", "Rustup toolchains", "Cadenas de herramientas de Rustup", "Rustup ツールチェーン", "Rustup 工具链", "Rustup 工具鏈")
k("data.item.rustupDownloads", "Rustup download cache", "Caché de descargas de Rustup", "Rustup ダウンロードキャッシュ", "Rustup 下载缓存", "Rustup 下載快取")
k("data.item.cargoRegistry", "Cargo dependency cache", "Caché de dependencias de Cargo", "Cargo 依存キャッシュ", "Cargo 依赖缓存", "Cargo 相依套件快取")
k("data.item.cargoGitDeps", "Cargo Git dependencies", "Dependencias Git de Cargo", "Cargo の Git 依存", "Cargo Git 依赖", "Cargo Git 相依")
k("data.item.cargoInstalledTools", "Cargo installed tools", "Herramientas instaladas con Cargo", "cargo install のツール", "Cargo 已装工具", "Cargo 已裝工具")
k("data.item.goModuleCache", "Module cache", "Caché de módulos", "モジュールキャッシュ", "模块缓存", "模組快取")
k("data.item.goBuildCache", "Build cache", "Caché de compilación", "ビルドキャッシュ", "构建缓存", "建置快取")
k("data.item.goInstalledTools", "Installed tools", "Herramientas instaladas", "インストール済みツール", "已装工具", "已裝工具")
k("data.item.brewDownloads", "Download cache", "Caché de descargas", "ダウンロードキャッシュ", "下载缓存", "下載快取")
k("data.item.vcpkgBinaryCache", "vcpkg binary cache", "Caché de binarios de vcpkg", "vcpkg バイナリキャッシュ", "vcpkg 二进制缓存", "vcpkg 二進位快取")
k("data.item.prismaEngines", "Prisma engines", "Motores de Prisma", "Prisma エンジン", "Prisma 引擎", "Prisma 引擎")
k("data.item.nugetGlobalPackages", "NuGet global packages", "Paquetes globales de NuGet", "NuGet グローバルパッケージ", "NuGet 全局包", "NuGet 全域套件")
k("data.item.nugetHttpCache", "NuGet HTTP cache", "Caché HTTP de NuGet", "NuGet HTTP キャッシュ", "NuGet HTTP 缓存", "NuGet HTTP 快取")
k("data.item.coursierLegacy", "Coursier (legacy)", "Coursier (heredado)", "Coursier（旧版）", "Coursier（旧版）", "Coursier（舊版）")
k("data.item.iosDeviceSymbols", "iOS device symbols", "Símbolos de dispositivos iOS", "iOS デバイスシンボル", "iOS 设备符号", "iOS 裝置符號")
k("data.item.simulatorRuntimes", "Simulator runtimes", "Tiempos de ejecución del simulador", "シミュレータランタイム", "模拟器运行时", "模擬器執行期")
k("data.item.xcodeCache", "Xcode cache", "Caché de Xcode", "Xcode キャッシュ", "Xcode 缓存", "Xcode 快取")
k("data.item.androidStudioCache", "Android Studio cache", "Caché de Android Studio", "Android Studio キャッシュ", "Android Studio 缓存", "Android Studio 快取")
k("data.item.devecoConfig", "DevEco Studio config", "Configuración de DevEco Studio", "DevEco Studio 設定", "DevEco Studio 配置", "DevEco Studio 設定")
k("data.item.commandLineTools", "Command line tools", "Herramientas de línea de comandos", "コマンドラインツール", "命令行工具", "命令列工具")
k("data.item.gradleLatest", "{version} (latest)", "{version} (más reciente)", "{version}（最新）", "{version}（最新）", "{version}（最新）")
k("data.item.gradleOlder", "{version} (older)", "{version} (anterior)", "{version}（旧版）", "{version}（旧版）", "{version}（舊版）")
k("data.item.mavenRest", "Other {count} entries", "Otros {count} elementos", "その他 {count} 件", "其它 {count} 项", "其他 {count} 項")

# ---- 工具名（品牌不翻，带中文后缀的那几档） ----
k("data.tool.jetbrainsProduct", "{name}", "{name}", "{name}", "{name}", "{name}")
k("data.tool.opencodeConfig", "opencode config", "Configuración de opencode", "opencode 設定", "opencode 配置", "opencode 設定")
k("data.tool.vscodeExtensions", "VS Code extensions", "Extensiones de VS Code", "VS Code 拡張機能", "VS Code 扩展", "VS Code 擴充功能")
k("data.tool.jetbrainsConfig", "JetBrains config", "Configuración de JetBrains", "JetBrains 設定", "JetBrains 配置", "JetBrains 設定")
k("data.tool.jetbrainsCache", "JetBrains cache", "Caché de JetBrains", "JetBrains キャッシュ", "JetBrains 缓存", "JetBrains 快取")
k("data.tool.deepseekDesktop", "DeepSeek Harness desktop", "Escritorio de DeepSeek Harness", "DeepSeek Harness デスクトップ版", "DeepSeek Harness 桌面版", "DeepSeek Harness 桌面版")
k("data.tool.warpConfig", "Warp config", "Configuración de Warp", "Warp 設定", "Warp 配置", "Warp 設定")
k("data.tool.warpLocalConfig", "Warp local config", "Configuración local de Warp", "Warp ローカル設定", "Warp 本地配置", "Warp 本機設定")
k("data.tool.zcodeDesktop", "ZCode desktop", "Escritorio de ZCode", "ZCode デスクトップ版", "ZCode 桌面版", "ZCode 桌面版")
k("data.tool.lingma", "Lingma", "Lingma", "Lingma", "灵码", "灵码")
k("data.tool.lingmaLocalData", "Lingma local data", "Datos locales de Lingma", "Lingma ローカルデータ", "灵码 本地数据", "灵码 本地資料")
k("data.tool.qoderDesktop", "Qoder desktop", "Escritorio de Qoder", "Qoder デスクトップ版", "Qoder 桌面版", "Qoder 桌面版")

# ---- 分节说明（notes） ----
k("data.note.reposNoHome",
  "Could not locate the home directory, so the scan roots can't be inferred",
  "No se pudo localizar la carpeta personal; no se pueden deducir las raíces de escaneo",
  "ホームディレクトリを特定できないため、スキャン元を推定できません",
  "未能定位用户主目录，无法推断扫描根目录",
  "未能定位使用者主目錄，無法推斷掃描根目錄")
k("data.note.reposRoots",
  "Scan roots: {roots}",
  "Raíces de escaneo: {roots}",
  "スキャン元ディレクトリ：{roots}",
  "扫描根目录：{roots}",
  "掃描根目錄：{roots}")
k("data.note.reposNested",
  "{count} nested repositories detected; a parent's size excludes its nested ones",
  "Se detectaron {count} repositorios anidados; el tamaño del padre excluye los hijos",
  "{count} 個の入れ子リポジトリを検出。親のサイズには子リポジトリを含めません",
  "检测到 {count} 个嵌套仓库，父仓库体积已排除子仓库部分",
  "偵測到 {count} 個巢狀倉庫，父倉庫體積已排除子倉庫部分")
k("data.note.aiTools.scope",
  "Only the whitelisted AI tool and editor data directories are scanned (see the SPECS table in ai_tools.rs); missing ones are skipped. The home directory is never searched and no file content is read",
  "Solo se escanean los directorios de datos de IA y editores de la lista blanca (ver la tabla SPECS en ai_tools.rs); los que no existen se omiten. No se busca en la carpeta personal ni se lee el contenido de archivos",
  "スキャン対象はホワイトリストに登録された AI ツールとエディタのデータディレクトリのみ（ai_tools.rs の SPECS 表を参照）。存在しなければスキップし、ホーム全体を探したりファイル内容を読んだりしません",
  "扫描位置仅限白名单中的 AI 工具与编辑器数据目录（见 ai_tools.rs 的 SPECS 表），不存在就跳过；不搜索整个主目录，不读取文件内容",
  "掃描位置僅限白名單中的 AI 工具與編輯器資料目錄（見 ai_tools.rs 的 SPECS 表），不存在就跳過；不搜尋整個主目錄，不讀取檔案內容")
k("data.note.aiTools.noContent",
  "Only file names and sizes are measured — no file content is read, so credentials never enter the scan result",
  "Solo se miden nombres de archivos y tamaños; no se lee ningún contenido, así que las credenciales nunca entran en el resultado",
  "ファイル名とサイズだけを計測し、内容は一切読みません。認証スキャン結果に憑拠は入りません",
  "只统计文件名与体积，不读取任何文件内容，凭据不会进入扫描结果",
  "只統計檔名與體積，不讀取任何檔案內容，憑證不會進入掃描結果")
k("data.note.aiTools.session",
  "Session data holds complete transcripts and code snippets — deleting it is irreversible, so decide what to keep first",
  "Los datos de sesión contienen conversaciones completas y fragmentos de código; borrarlos es irreversible, confirma antes qué conservar",
  "セッションデータには完全な会話記録とコード片が含まれます。削除は元に戻せないため、残すかどうか先に確認してください",
  "会话数据含完整对话记录与代码片段，删除不可逆，清理前先确认还要不要留",
  "工作階段資料含完整對話紀錄與程式碼片段，刪除不可逆，清理前先確認是否保留")
k("data.note.aiTools.index",
  "Index categories include SQLite database files (Codex's logs_2.sqlite is named a log but is a database) — deleting them directly can corrupt the tool's state",
  "La categoría de índices incluye bases de datos SQLite (logs_2.sqlite de Codex se llama registro pero es una base de datos); borrarlas directamente puede dañar el estado de la herramienta",
  "インデックス類には SQLite データベースが含まれます（Codex の logs_2.sqlite は名前はログ、実体は DB）。直接削除するとツールの状態を壊す恐れがあります",
  "索引类含 SQLite 数据库文件（Codex 的 logs_2.sqlite 名义是日志、载体是数据库），直接删除可能损坏工具状态",
  "索引類含 SQLite 資料庫檔（Codex 的 logs_2.sqlite 名義是日誌、載體是資料庫），直接刪除可能損壞工具狀態")
k("data.note.aiTools.skill",
  "Skills and plugins have to be reinstalled after deletion, so cleaning is not offered for them",
  "Habilidades y complementos hay que reinstalarlos tras borrarlos, así que aquí no se ofrece limpiarlos",
  "スキルとプラグインは削除後に再インストールが必要なため、クリーンアップは開放していません",
  "技能与插件删除后需重新安装，本工具不开放清理",
  "技能與外掛刪除後需重新安裝，本工具不開放清理")
k("data.note.aiTools.unknown",
  "{count} unrecognised entries (total {bytes}) are grouped as Unknown — none of them can be cleaned",
  "{count} elementos no reconocidos ({bytes} en total) se agrupan como «Desconocido»; ninguno se puede limpiar",
  "認識できない {count} 件（合計 {bytes}）は「不明」にまとめ、クリーンアップ対象にしません",
  "未识别 {count} 项（合计 {bytes}）归入「未知」，一律不开放清理",
  "未識別 {count} 項（合計 {bytes}）歸入「未知」，一律不開放清理")
k("data.note.aiTools.symlinks",
  "{count} symlinks were skipped and never followed",
  "Se omitieron {count} enlaces simbólicos; no se siguen al calcular",
  "{count} 個のシンボリックリンクをスキップし、追跡して計測していません",
  "跳过 {count} 个符号链接，不跟随统计",
  "跳過 {count} 個符號連結，不跟隨統計")
k("data.note.toolchain.noContent",
  "Only file names and sizes are measured — no file content is read",
  "Solo se miden nombres de archivos y tamaños; no se lee ningún contenido",
  "ファイル名とサイズだけを計測し、ファイル内容は読みません",
  "只统计文件名与体积，不读取任何文件内容",
  "只統計檔名與體積，不讀取任何檔案內容")
k("data.note.toolchain.reclaimRule",
  "Reclaimable means a pure download or dependency cache with no official cleanup command: deleting it only costs a re-download or rebuild. Where an official command exists you run it yourself (see each row's cleanup method) — this tool does not run it and does not count it as reclaimable",
  "Recuperable significa caché pura de descargas o dependencias sin comando oficial de limpieza: borrarla solo cuesta una descarga o reconstrucción. Si existe comando oficial, lo ejecutas tú (mira el método de limpieza de cada fila); esta herramienta no lo ejecuta ni lo cuenta como recuperable",
  "回収可能とは、公式のクリーンアップコマンドを持たないダウンロード／依存キャッシュのことです。削除しても再ダウンロードまたは再構築で戻ります。公式コマンドがあるものは各自で実行してください（各行の「クリーンアップ方法」を参照）。本ツールは代行も計上もしません",
  "可回收口径：没有官方清理命令的纯下载/依赖缓存，删除后按需重新下载或重建，功能无损；有官方清理命令的由你自己运行该命令（见每行「清理方式」），本工具不代跑、不计入可回收",
  "可回收口徑：沒有官方清理命令的純下載／相依快取，刪除後按需重新下載或重建，功能無損；有官方清理命令的由你自己執行該命令（見每行「清理方式」），本工具不代跑、不計入可回收")
k("data.note.toolchain.installedNotReclaimable",
  "Installed toolchains and binaries are never reclaimable — which versions you keep is a deliberate choice",
  "Las toolchains y binarios instalados nunca son recuperables; qué versiones conservar es una decisión tuya",
  "インストール済みのツールチェーンとバイナリは回収可能に計上しません。どの版本を残すかは能動的な選択です",
  "已安装的工具链与二进制不计入可回收，留哪个版本是主动选择",
  "已安裝的工具鏈與二進位不計入可回收，留哪個版本是主動選擇")
k("data.note.toolchain.waitBuilds",
  "Cleanup follows each tool's official command; if a build or download is running, wait for it to finish first",
  "La limpieza sigue el comando oficial de cada herramienta; si hay una compilación o descarga en curso, espera a que termine",
  "クリーンアップは各ツールの公式コマンドに準拠します。ビルドやダウンロードが進行中なら完了を待ってください",
  "清理命令以各工具官方命令为准；有进行中的构建或下载时建议等完成再清理",
  "清理命令以各工具官方命令為準；有進行中的建置或下載時建議等完成再清理")
k("data.note.toolchain.xcodeArchives",
  "Xcode Archives are shipped builds, not caches, and are never reclaimable; a deleted simulator runtime has to be re-downloaded (official command in the details)",
  "Los Archives de Xcode son archivos de publicación, no caché, y nunca son recuperables; un tiempo de ejecución del simulador borrado hay que descargarlo de nuevo (comando oficial en el detalle)",
  "Xcode の Archives はビルド归档であってキャッシュではないため、回収可能に計上しません。削除したシミュレータランタイムは再ダウンロードが必要です（公式コマンドは明細にあり）",
  "Xcode 的 Archives 是打包归档不是缓存，永不计入可回收；模拟器运行时删了要重新下载（官方命令见明细）",
  "Xcode 的 Archives 是打包歸檔不是快取，永不計入可回收；模擬器執行期刪了要重新下載（官方命令見明細）")
k("data.note.toolchain.versionManagers",
  "Version managers ({managers}) list one row per installed version: keeping several is deliberate, none are reclaimable, and each uninstalls with its own command",
  "Los gestores de versiones ({managers}) muestran una fila por versión instalada: tener varias es deliberado, ninguna es recuperable y cada una se desinstala con su propio comando",
  "バージョンマネージャ（{managers}）はインストール済みの版本ごとに 1 行です。複数併存は能動的な選択で、いずれも回収不可、卸载は各自のコマンドで行います",
  "版本管理器（{managers}）逐版本成行：多版本并存是主动选择，全部不可回收，卸掉用各家自己的 uninstall 命令",
  "版本管理器（{managers}）逐版本成行：多版本並存是主動選擇，全部不可回收，卸除用各家自己的 uninstall 命令")
k("data.note.toolchain.versionManagersNotReclaimable",
  "Version managers never report a reclaimable amount: this tool cannot tell which project uses which version (that would mean reading every repository's .nvmrc / engines and intersecting them), and a number here would get a still-used version deleted",
  "Los gestores de versiones nunca indican una cantidad recuperable: esta herramienta no sabe qué proyecto usa qué versión (haría falta leer .nvmrc / engines de todos los repositorios e intersectarlos) y un número aquí llevaría a borrar una versión aún en uso",
  "バージョンマネージャは回収可能量を報告しません：どのプロジェクトがどの版本を使っているか本ツールでは判定できません（各リポジトリの .nvmrc / engines を読んで積集合を取る必要がある）。数値を出せば現に使用中の版本を消す原因になります",
  "版本管理器不报「可回收」：本工具读不到哪个项目正在用哪个版本（要读各仓库的 .nvmrc / engines 字段并逐个求交集），报一个数会让人照着删掉还在用的版本",
  "版本管理器不報「可回收」：本工具讀不到哪個專案正在用哪個版本（要讀各倉庫的 .nvmrc / engines 欄位並逐個求交集），報一個數會讓人照著刪掉還在用的版本")
k("data.note.toolchain.empty",
  "None of the whitelisted cache directories exist; the matching tools may not be installed",
  "No existe ninguno de los directorios de caché de la lista blanca; quizá no estén instaladas las herramientas",
  "ホワイトリスト内のキャッシュディレクトリが一つも存在しません。対応するツールが未インストールの可能性があります",
  "白名单内的缓存目录一个都不存在，可能未安装对应工具",
  "白名單內的快取目錄一個都不存在，可能未安裝對應工具")
k("data.note.devEnv.noContent",
  "Only file names and sizes are measured — no file content is read",
  "Solo se miden nombres de archivos y tamaños; no se lee ningún contenido",
  "ファイル名とサイズだけを計測し、ファイル内容は読みません",
  "只统计文件名与体积，不读取任何文件内容",
  "只統計檔名與體積，不讀取任何檔案內容")
k("data.note.devEnv.allMissing",
  "None of the {count} locations above exist; this environment may not be installed",
  "Ninguna de las {count} ubicaciones anteriores existe; quizá no esté instalado este entorno",
  "上記 {count} 箇所のいずれも存在しません。この環境は未インストールの可能性があります",
  "以上 {count} 个位置都不存在，环境可能未安装",
  "以上 {count} 個位置都不存在，環境可能未安裝")
k("data.note.java.mavenRebuildable",
  "The Maven dependency cache can be rebuilt from the network, but deleting it means re-downloading, so it is not counted as reclaimable",
  "La caché de dependencias de Maven puede reconstruirse desde la red, pero borrarla implica descargar de nuevo, así que no cuenta como recuperable",
  "Maven の依存キャッシュはネットワークから再構築できますが、削除すると再ダウンロードが必要なため回収可能に計上しません",
  "Maven 依赖缓存可从网络重建，但删掉要重新下载，不计入可回收",
  "Maven 相依快取可從網路重建，但刪掉要重新下載，不計入可回收")
k("data.note.java.multipleJdks",
  "Several JDKs side by side isn't leftover here: 17 and 21 are both mainstream LTS releases and projects need different ones — which to keep is your choice",
  "Varios JDK a la vez no son sobras aquí: 17 y 21 son LTS mayoritarios y cada proyecto pide uno distinto; cuál conservar lo decides tú",
  "複数の JDK の併存は残骸ではありません。17 と 21 はともに主流の LTS で、プロジェクトごとの要求が異なるため、残すものは自分の判断です",
  "多个 JDK 并存在这里不算残留：17 与 21 都是主流长期支持版，不同项目要求不同，留谁是主动选择",
  "多個 JDK 並存在這裡不算殘留：17 與 21 都是主流長期支援版，不同專案要求不同，留誰是主動選擇")
k("data.note.java.gradleOlder",
  "Old Gradle distributions and daemon logs are marked reclaimable; until cleaning is enabled here they are only shown, never deleted",
  "Las distribuciones antiguas de Gradle y los registros del demonio se marcan como recuperables; mientras no se active la limpieza aquí solo se muestran, nunca se borran",
  "古い Gradle ディストリビューションとデーモンログは回収可能にマークしていますが、清理エントリを有効にするまでは表示のみで削除しません",
  "Gradle 旧分发版与守护日志标为可回收，清理入口启用前只显示不删除",
  "Gradle 舊發行版與守護日誌標為可回收，清理入口啟用前只顯示不刪除")
k("data.note.localModels.assetNotCache",
  "Models are assets, not caches: deleting one means re-downloading tens of gigabytes, so this group never marks anything reclaimable",
  "Los modelos son activos, no caché: borrar uno implica descargar decenas de gigabytes, así que este grupo nunca marca nada como recuperable",
  "モデルはキャッシュではなく資産です。1 つ消すと数十 GB 再ダウンロードが必要なため、このグループは回収可能を一切付けません",
  "模型是资产不是缓存：删一个要重新下载（几十 GB 起步），所以本组永不标可回收",
  "模型是資產不是快取：刪一個要重新下載（數十 GB 起），所以本組永不標可回收")
k("data.note.localModels.ollamaLayers",
  "Ollama resolves the weight layers each model references through its manifest; when a layer file is missing only what's on disk is counted, and the missing count is in the details",
  "Ollama resuelve mediante el manifest las capas de pesos que referencia cada modelo; si falta alguna solo se cuenta lo que hay en disco y el número de faltantes aparece en el detalle",
  "Ollama は manifest を読んで各モデルが参照する重みレイヤーを解決します。レイヤーファイルが無ければディスク上の分だけ計測し、欠損数は明細に書きます",
  "Ollama 按 manifest 解析每个模型引用的权重层；权重文件缺失时只计磁盘上存在的部分，缺失层数写在明细里",
  "Ollama 依 manifest 解析每個模型引用的權重層；權重檔缺失時只計磁碟上存在的部分，缺失層數寫在明細裡")
k("data.note.localModels.sharedLayers",
  "When a model has several tags its size is counted once per reference; what deleting a single tag actually frees is in the shared-layers row of the details (only shown when layers really are shared)",
  "Si un modelo tiene varias etiquetas su tamaño se cuenta una vez por referencia; lo que libera borrar una etiqueta está en la fila «capas compartidas» del detalle (solo aparece si hay capas compartidas)",
  "同一モデルに複数の tag がある場合、體積は参照ごとに 1 回計数します。1 つの tag を消して実際に空く容量は明細の「共有レイヤー」行にあり（共有がある時だけ表示）",
  "同一模型装了多个 tag 时体积按引用各计一次；删掉单个 tag 实际腾出多少看明细的「共享层」一行（只在真有共享时出现）",
  "同一模型裝了多個 tag 時體積按引用各計一次；刪掉單個 tag 實際騰出多少看明細的「共享層」一行（只在真有共享時出現）")
k("data.note.localModels.lmstudioScope",
  "LM Studio scans only models/ and hub/models/ — the session, log and credential directories under ~/.lmstudio aren't model assets and are not listed",
  "LM Studio solo escanea models/ y hub/models/; las carpetas de sesiones, registros y credenciales bajo ~/.lmstudio no son activos de modelos y no se enumeran",
  "LM Studio は models/ と hub/models/ のみをスキャンします。~/.lmstudio 配下のセッション・ログ・認証ディレクトリはモデル資産ではないため列挙しません",
  "LM Studio 只扫 models/ 与 hub/models/——~/.lmstudio 下的会话、日志与凭据目录不是模型资产，本工具不枚举",
  "LM Studio 只掃 models/ 與 hub/models/——~/.lmstudio 下的工作階段、日誌與憑證目錄不是模型資產，本工具不列舉")
k("data.note.localModels.huggingfaceScope",
  "HuggingFace is measured per model directory (snapshots are symlinks into blobs, so nothing is double-counted)",
  "HuggingFace se mide por directorio de modelo (snapshots son enlaces a blobs, así que nada se cuenta dos veces)",
  "HuggingFace はモデルディレクトリ単位で計測します（snapshots は blobs へのシンボリックリンクなので二重計上しません）",
  "HuggingFace 按模型目录整体计（snapshots 是指向 blobs 的软链，不重复计）",
  "HuggingFace 按模型目錄整體計（snapshots 是指向 blobs 的軟鏈，不重複計）")
k("data.note.wsl.sizeMeaning",
  "The size is the space taken by the ext4.vhdx file, not how much the distro actually uses",
  "El tamaño es el espacio que ocupa el archivo ext4.vhdx, no lo que la distribución usa de verdad",
  "表示しているのは ext4.vhdx ファイルの占有容量で、ディストリビューション内部の実使用量ではありません",
  "体积是 ext4.vhdx 文件的占用空间，不是发行版里实际用了多少",
  "體積是 ext4.vhdx 檔的佔用空間，不是發行版裡實際用了多少")
k("data.note.wsl.growsOnly",
  "A dynamically expanding vhdx only grows: deleting files inside the distro leaves holes in the virtual disk while the file on the Windows side doesn't shrink — which is why no reclaimable number can be given here",
  "Un vhdx de expansión dinámica solo crece: borrar archivos dentro de la distribución deja huecos en el disco virtual y el archivo del lado de Windows no se reduce, así que aquí no se puede dar una cifra recuperable",
  "動的拡張の vhdx は伸びるだけで縮みません。ディストリビューション内でファイルを消しても仮想ディスク内に空洞が残るだけで、Windows 側のファイルサイズは変わらないため、「消すとどれだけ空く」を出せません",
  "动态扩展的 vhdx 只涨不缩：在发行版里删文件只在虚拟盘内部留空洞，Windows 侧文件尺寸纹丝不动——所以这里没法给出「删了能省多少」",
  "動態擴展的 vhdx 只漲不縮：在發行版裡刪檔案只在虛擬盤內部留空洞，Windows 側檔案尺寸紋風不動——所以這裡沒辦法給出「刪了能省多少」")
k("data.note.wsl.howToShrink",
  "Handing the space back to Windows takes three steps: sudo fstrim -av inside the distro, then wsl --shutdown, then Optimize-VHD -Mode Full (or diskpart's compact vdisk) as administrator. This tool does none of them",
  "Devolver el espacio a Windows requiere tres pasos: sudo fstrim -av dentro de la distribución, luego wsl --shutdown y después Optimize-VHD -Mode Full (o compact vdisk en diskpart) como administrador. Esta herramienta no hace ninguno",
  "Windows に容量を戻すには 3 段階が必要です：ディストリビューション内で sudo fstrim -av → wsl --shutdown → 管理者で Optimize-VHD -Mode Full（または diskpart の compact vdisk）。本ツールはいずれも実行しません",
  "要把空间还给 Windows 得三步：发行版内 sudo fstrim -av → wsl --shutdown → 管理员执行 Optimize-VHD -Mode Full（或 diskpart 的 compact vdisk）。本工具一条都不做",
  "要把空間還給 Windows 得三步：發行版內 sudo fstrim -av → wsl --shutdown → 管理者執行 Optimize-VHD -Mode Full（或 diskpart 的 compact vdisk）。本工具一條都不做")
k("data.note.wsl.gapIsNormal",
  "How big the gap can be is normal: one measured vhdx held 12 GB of data inside a 60 GB file, another showed 583 GB in Windows while df inside the distro reported 93 GB",
  "Que la diferencia sea grande es normal: en una prueba un vhdx de 60 GB guardaba 12 GB de datos reales y en otro Windows mostraba 583 GB mientras df dentro de la distribución marcaba 93 GB",
  "差が大きくて普通です。実測では 60 GB の vhdx に実際のデータが 12 GB、別の例では Windows 表示 583 GB に対してディストリビューション内の df は 93 GB でした",
  "差多少属正常：实测有 60 GB 的 vhdx 里只有 12 GB 实际数据，另一例 Windows 显示 583 GB 而发行版内 df 只有 93 GB",
  "差多少屬正常：實測有 60 GB 的 vhdx 裡只有 12 GB 實際資料，另一例 Windows 顯示 583 GB 而發行版內 df 只有 93 GB")
k("data.note.wsl.registryOnly",
  "Locations are read from the Lxss registry key (wsl --list -v doesn't give paths); no wsl command is run and the distro is never entered",
  "Las ubicaciones se leen de la clave de registro Lxss (wsl --list -v no da rutas); no se ejecuta ningún comando wsl ni se entra en la distribución",
  "位置はレジストリの Lxss キーから読みます（wsl --list -v はパスをくれないため）。wsl コマンドは実行せず、ディストリビューション内部にも入りません",
  "位置读自注册表 Lxss 键（wsl --list -v 不给路径），不执行任何 wsl 命令，也不进入发行版内部",
  "位置讀自註冊表 Lxss 鍵（wsl --list -v 不給路徑），不執行任何 wsl 命令，也不進入發行版內部")

# ---- 分组摘要（summary） ----
k("data.summary.repos",
  "{count} repositories · total {total} · {clean} reclaimable ({reclaim})",
  "{count} repositorios · total {total} · {clean} recuperables ({reclaim})",
  "{count} 個のリポジトリ · 合計 {total} · {clean} 件が回収可能（{reclaim}）",
  "{count} 个仓库 · 合计 {total} · {clean} 个可回收 · 共 {reclaim}",
  "{count} 個倉庫 · 合計 {total} · {clean} 個可回收 · 共 {reclaim}")
k("data.summary.reposEmpty", "No repositories found", "No se encontró ningún repositorio", "リポジトリが見つかりません", "未找到仓库", "未找到倉庫")
k("data.summary.reposNoRoots", "No scan roots configured", "Sin raíces de escaneo configuradas", "スキャン元が未設定", "未配置根目录", "未配置根目錄")
k("data.summary.toolchain",
  "{count} entries · total {total} · {clean} reclaimable ({reclaim})",
  "{count} elementos · total {total} · {clean} recuperables ({reclaim})",
  "{count} 件 · 合計 {total} · {clean} 件が回収可能（{reclaim}）",
  "{count} 项 · 合计 {total} · {clean} 项可回收 · 共 {reclaim}",
  "{count} 項 · 合計 {total} · {clean} 項可回收 · 共 {reclaim}")
k("data.summary.aiTools",
  "{locations} locations · {items} entries · total {total} · {clean} reclaimable ({reclaim})",
  "{locations} ubicaciones · {items} elementos · total {total} · {clean} recuperables ({reclaim})",
  "{locations} 箇所 · {items} 件 · 合計 {total} · {clean} 件が回収可能（{reclaim}）",
  "{locations} 个位置 · {items} 个条目 · 合计 {total} · {clean} 项可回收 · 共 {reclaim}",
  "{locations} 個位置 · {items} 個條目 · 合計 {total} · {clean} 項可回收 · 共 {reclaim}")
k("data.summary.aiToolsEmpty",
  "No AI tool or editor data directories found",
  "No se encontraron directorios de datos de IA ni de editores",
  "AI ツールとエディタのデータディレクトリが見つかりません",
  "未找到 AI 工具或编辑器数据目录",
  "未找到 AI 工具或編輯器資料目錄")
k("data.summary.devEnv",
  "{count} entries · total {total} · {clean} reclaimable ({reclaim})",
  "{count} elementos · total {total} · {clean} recuperables ({reclaim})",
  "{count} 件 · 合計 {total} · {clean} 件が回収可能（{reclaim}）",
  "{count} 项 · 合计 {total} · {clean} 项可回收 · 共 {reclaim}",
  "{count} 項 · 合計 {total} · {clean} 項可回收 · 共 {reclaim}")
k("data.summary.localModels",
  "{count} models · total {total}",
  "{count} modelos · total {total}",
  "{count} 個のモデル · 合計 {total}",
  "{count} 个模型 · 合计 {total}",
  "{count} 個模型 · 合計 {total}")
k("data.summary.wsl",
  "{count} entries · total {total} · a vhdx only grows, so nothing here is reclaimable",
  "{count} elementos · total {total} · un vhdx solo crece, así que aquí nada es recuperable",
  "{count} 件 · 合計 {total} · vhdx は伸びるだけで縮まらないため、回収可能は計上しません",
  "{count} 项 · 合计 {total} · vhdx 只涨不缩，本组不计可回收",
  "{count} 項 · 合計 {total} · vhdx 只漲不縮，本組不計可回收")
k("data.summary.overview",
  "{count} groups · total {total} · {reclaim} reclaimable",
  "{count} grupos · total {total} · {reclaim} recuperable",
  "{count} 個のグループ · 合計 {total} · 回収可能 {reclaim}",
  "{count} 个分组 · 合计 {total} · 可回收 {reclaim}",
  "{count} 個分組 · 合計 {total} · 可回收 {reclaim}")
k("data.summary.sshMissingDir", "No .ssh directory found", "No se encontró la carpeta .ssh", ".ssh ディレクトリが見つかりません", "未找到 .ssh 目录", "未找到 .ssh 目錄")
k("data.summary.sshReadFailed", "Could not read", "No se pudo leer", "読み取りに失敗", "读取失败", "讀取失敗")

# ---- 指标值里的固定取值 ----
k("data.value.noCommit", "No commits", "Sin commits", "コミットなし", "无提交", "無提交")
k("data.value.detachedBranch", "Detached", "Desasociada", "デタッチ", "游离", "分離")
k("data.value.unknown", "Unknown", "Desconocido", "不明", "未知", "未知")

# ---- 错误 ----
k("data.error.repoRootsChanged",
  "Scan directories changed — rescan Git repositories",
  "Las carpetas de escaneo cambiaron: vuelve a escanear los repositorios Git",
  "スキャンディレクトリが変わりました。Git リポジトリを再スキャンしてください",
  "扫描目录已更改，请重扫 Git 仓库",
  "掃描目錄已更改，請重掃 Git 倉庫")
k("data.error.repoRootsRead",
  "Could not read the extra Git repository directories: {error}",
  "No se pudieron leer las carpetas adicionales de repositorios Git: {error}",
  "Git リポジトリの追加ディレクトリを読み取れません：{error}",
  "无法读取 Git 仓库补充目录：{error}",
  "無法讀取 Git 倉庫補充目錄：{error}")


def block(locale):
    idx = {"en": 0, "es": 1, "ja": 2, "zh-CN": 3, "zh-TW": 4}[locale]
    lines = []
    prev = ""
    for key, vals in D.items():
        group = key.split(".")[1]
        if prev and group != prev:
            lines.append("")
        prev = group
        lines.append("  %s: %s," % (json.dumps(key, ensure_ascii=False), json.dumps(vals[idx], ensure_ascii=False)))
    lines[-1] = lines[-1].rstrip(",")
    return "\n".join(lines)


def main():
    for locale in ["en", "es", "ja", "zh-CN", "zh-TW"]:
        p = "src/locales/%s.json" % locale
        raw = io.open(p, encoding="utf-8").read()
        json.loads(raw)  # sanity
        if re.search(r'"data\.', raw):
            print("skip (already has data.*):", p)
            continue
        assert raw.rstrip().endswith("}"), p
        body = raw.rstrip()[:-1].rstrip("\n")
        lines = body.split("\n")
        assert lines[-1].rstrip().endswith('"'), lines[-1]
        lines[-1] = lines[-1].rstrip() + ","
        out = "\n".join(lines) + "\n\n" + block(locale) + "\n}\n"
        json.loads(out)
        io.open(p, "w", encoding="utf-8").write(out)
        print("wrote %d data keys -> %s" % (len(D), p))


main()
