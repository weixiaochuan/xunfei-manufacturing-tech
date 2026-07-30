import { create } from "zustand";
import {
  accountApi,
  type AccountLoginResult,
  type DesktopAccountUser,
} from "@/lib/api";
import { changeDocumentAccount } from "@/lib/documents/documentSession";
import { useTabsStore } from "@/store/tabs";
import { usePptGenerationDraftStore } from "@/store/pptGenerationDraft";

function clearAccountScopedResources(): void {
  useTabsStore.getState().closeAllTabs();
  usePptGenerationDraftStore.getState().clearInternalMaterial();
}

export type AccountLoginStatus =
  | "signedOut"
  | "restoring"
  | "waiting"
  | "signedIn"
  | "signingOut"
  | "unavailable"
  | "error";

interface AccountStore {
  currentUser: DesktopAccountUser | null;
  loginStatus: AccountLoginStatus;
  loginError: string | null;
  beginLogin: () => Promise<void>;
  restoreSession: () => Promise<AccountLoginResult>;
  logout: () => Promise<AccountLoginResult>;
  applyLoginResult: (result: AccountLoginResult) => void;
}

export const useAccountStore = create<AccountStore>((set, get) => ({
  currentUser: null,
  loginStatus: "signedOut",
  loginError: null,

  beginLogin: async () => {
    if (get().loginStatus === "waiting") {
      return;
    }
    set({ loginStatus: "waiting", loginError: null });
    try {
      await accountApi.beginLogin();
    } catch {
      set({
        loginStatus: "error",
        loginError: "无法打开系统浏览器，请稍后重试",
      });
    }
  },

  restoreSession: async () => {
    set({ loginStatus: "restoring", loginError: null });
    try {
      const result = await accountApi.restoreSession();
      get().applyLoginResult(result);
      return result;
    } catch {
      const result: AccountLoginResult = {
        status: "unavailable",
        message: "账号服务暂不可用，本地功能仍可正常使用",
      };
      get().applyLoginResult(result);
      return result;
    }
  },

  logout: async () => {
    set({ loginStatus: "signingOut", loginError: null });
    try {
      const result = await accountApi.logout();
      get().applyLoginResult(result);
      return result;
    } catch {
      const result: AccountLoginResult = {
        status: "error",
        message: "退出失败，请稍后重试",
      };
      get().applyLoginResult(result);
      return result;
    }
  },

  applyLoginResult: (result) => {
    if (result.status === "success") {
      changeDocumentAccount(result.user.platformUserId);
      clearAccountScopedResources();
      set({
        currentUser: result.user,
        loginStatus: "signedIn",
        loginError: null,
      });
      return;
    }
    if (result.status === "signedOut") {
      changeDocumentAccount(null);
      clearAccountScopedResources();
      set({ currentUser: null, loginStatus: "signedOut", loginError: null });
      return;
    }
    if (result.status === "unavailable") {
      changeDocumentAccount(null);
      clearAccountScopedResources();
      set({
        currentUser: null,
        loginStatus: "unavailable",
        loginError: result.message,
      });
      return;
    }
    changeDocumentAccount(null);
    clearAccountScopedResources();
    set({
      currentUser: null,
      loginStatus: "error",
      loginError: result.message || "登录失败，请重试",
    });
  },
}));
