import { getAppRpc } from "./app-rpc";

export type { AppUpdateStage, AppUpdateState } from "../shared/app-rpc";

export async function getAppUpdateState() {
  const rpc = await getAppRpc();
  return rpc?.request.getAppUpdateState();
}

export async function checkForAppUpdate() {
  const rpc = await getAppRpc();
  return rpc?.request.checkForAppUpdate();
}

export async function downloadAppUpdate() {
  const rpc = await getAppRpc();
  return rpc?.request.downloadAppUpdate();
}

export async function applyAppUpdate() {
  const rpc = await getAppRpc();
  return rpc?.request.applyAppUpdate();
}
