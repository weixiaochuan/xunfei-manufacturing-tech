import { useEffect } from "react";
import { Button, Dropdown, Tooltip, message } from "antd";
import { FolderOpenOutlined, LogoutOutlined, UserOutlined } from "@ant-design/icons";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { useNavigate } from "react-router-dom";
import { accountApi, type AccountLoginResult } from "@/lib/api";
import { useAccountStore } from "@/store/account";

const ACCOUNT_LOGIN_EVENT = "account:login-result";

export function AccountStatusButton() {
  const navigate = useNavigate();
  const {
    currentUser,
    loginStatus,
    loginError,
    beginLogin,
    restoreSession,
    logout,
    applyLoginResult,
  } = useAccountStore();

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    const apply = (result: AccountLoginResult) => {
      applyLoginResult(result);
      if (result.status === "error") {
        message.error(result.message || "登录失败，请重试");
      }
    };

    void listen<AccountLoginResult>(ACCOUNT_LOGIN_EVENT, (event) => {
      apply(event.payload);
    }).then(async (stopListening) => {
      if (disposed) {
        stopListening();
        return;
      }
      unlisten = stopListening;
      try {
        const pending = await accountApi.takePendingResult();
        if (pending && !disposed) {
          apply(pending);
        } else if (!disposed) {
          const restored = await restoreSession();
          if (!disposed && restored.status === "unavailable") {
            message.warning(restored.message);
          }
        }
      } catch {
        if (!disposed) {
          const restored = await restoreSession();
          if (!disposed && restored.status === "unavailable") {
            message.warning(restored.message);
          }
        }
      }
    });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [applyLoginResult, restoreSession]);

  if ((loginStatus === "signedIn" || loginStatus === "signingOut") && currentUser) {
    const items = [
      { key: "account", label: `平台账号：${currentUser.accountNumber}`, disabled: true },
      { key: "username", label: `用户名：${currentUser.username}`, disabled: true },
      ...(currentUser.email
        ? [{ key: "email", label: `邮箱：${currentUser.email}`, disabled: true }]
        : []),
      { type: "divider" as const },
      { key: "files", icon: <FolderOpenOutlined />, label: "文档" },
      { key: "logout", icon: <LogoutOutlined />, label: "退出登录" },
    ];
    return (
      <Dropdown
        menu={{
          items,
          onClick: ({ key }) => {
            if (key === "files") {
              navigate("/notes");
              return;
            }
            if (key === "logout") {
              void logout().then((result) => {
                if (result.status === "signedOut") {
                  message.success("已退出登录");
                } else if (result.status === "error") {
                  message.error(result.message);
                }
              });
            }
          },
        }}
        trigger={["click"]}
      >
        <Button
          type="text"
          icon={<UserOutlined />}
          style={{ height: 44 }}
          loading={loginStatus === "signingOut"}
          disabled={loginStatus === "signingOut"}
        >
          <span style={{ display: "inline-flex", flexDirection: "column", lineHeight: 1.15 }}>
            <span>{currentUser.displayName || currentUser.username}</span>
            <span style={{ fontSize: 11, opacity: 0.68 }}>{currentUser.accountNumber}</span>
          </span>
        </Button>
      </Dropdown>
    );
  }

  const waiting = loginStatus === "waiting" || loginStatus === "restoring";
  const label =
    loginStatus === "restoring"
      ? "正在恢复账号"
      : loginStatus === "waiting"
        ? "等待浏览器登录"
        : loginStatus === "error" || loginStatus === "unavailable"
          ? "重新登录"
          : "登录";
  const button = (
    <Button
      type="text"
      icon={<UserOutlined />}
      loading={waiting}
      disabled={waiting}
      onClick={() => void beginLogin()}
    >
      {label}
    </Button>
  );

  return loginError ? <Tooltip title={loginError}>{button}</Tooltip> : button;
}
