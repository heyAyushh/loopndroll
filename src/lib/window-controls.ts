import type { WindowControlAction } from "../shared/app-rpc";
import { getAppRpc } from "./app-rpc";

export async function getWindowControlsState() {
  const rpc = await getAppRpc();
  return rpc?.request.getWindowState();
}

export async function runWindowControl(action: WindowControlAction) {
  const rpc = await getAppRpc();
  return rpc?.request.windowControl({ action });
}
