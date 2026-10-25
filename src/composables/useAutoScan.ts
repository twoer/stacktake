import { ref } from "vue";
import { AUTO_SCAN_STORAGE_KEY, parseAutoScanPreference } from "@/lib/autoScan";

/**
 * 启动自动重扫的开关。模块级单例，与 useTheme 同一套：设置页写入，
 * useAssets 在 boot 时读同一份，不需要跨页传值。
 *
 * 缺省开启是刻意的——只有用户显式取消才落盘 "0"。localStorage 读不出来
 * （隐私模式等）也按开启处理，与「没选过」同义。
 */
const enabled = ref(true);
try {
  enabled.value = parseAutoScanPreference(localStorage.getItem(AUTO_SCAN_STORAGE_KEY));
} catch {
  /* 读不到就保持默认开启 */
}

export function useAutoScan() {
  function setEnabled(next: boolean) {
    enabled.value = next;
    try {
      localStorage.setItem(AUTO_SCAN_STORAGE_KEY, next ? "1" : "0");
    } catch {
      /* 存不下不影响本次会话 */
    }
  }

  return { enabled, setEnabled };
}
