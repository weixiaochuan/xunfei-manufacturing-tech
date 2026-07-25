# Casdoor TEST 独立环境部署

该环境仅用于 AI 助学及其他功能的账号联调。它只包含独立的 PostgreSQL 和 Casdoor，不包含 Account Server、Caddy 或公网入口，也不读取正式 Cloud 环境文件。

## 隔离边界

| 资源 | Casdoor TEST |
|------|--------------|
| Compose project | `pomegranate-casdoor-test` |
| PostgreSQL service | `postgres-test` |
| Casdoor service | `casdoor-test` |
| PostgreSQL database | `casdoor_test` |
| PostgreSQL application role | `casdoor_test_app` |
| Named volume | `pomegranate_casdoor_test_postgres_data` |
| Network | `pomegranate_casdoor_test_backend` |
| Casdoor image | `casbin/casdoor:3.119.0` |
| Host binding | `127.0.0.1:18000` |
| Organization | `pomegranate-test` |
| Application | `app-pomegranate-test` |

测试栈不得加入 `pomegranate_backend`、`pomegranate_edge`，不得挂载正式 PostgreSQL volume，也不得读取 `.env.cloud`。测试 Casdoor 的数据库密码、管理员密码和后续创建的 Client Secret 必须全部重新生成。

当前 Casdoor 镜像没有单独的持久化数据挂载；Casdoor 数据保存在独立的 `postgres-test` volume 中。不要猜测镜像内部路径并挂载正式 Casdoor 数据。

## 创建私密环境文件

在包含 `compose.casdoor-test.yml` 的项目目录执行：

```bash
cp .env.casdoor-test.example .env.casdoor-test
chmod 600 .env.casdoor-test
```

分别生成两个不同的随机密码：

```bash
openssl rand -hex 32
openssl rand -hex 32
```

替换 `.env.casdoor-test` 中所有 `CHANGE_ME` 值。真实环境文件不得提交、发送到聊天或与 `.env.cloud` 合并。

Casdoor Application 的 Client Secret 不属于本 Compose。首次启动后，应在 Casdoor 管理页面创建测试 Application，并把 Secret 仅写入使用它的测试后端私密环境文件。

## 静态验证

静态检查不会启动容器：

```bash
bash ./scripts/casdoor-test/validate-casdoor-test.sh
```

它会拒绝占位密码、共享密码、错误数据库名称、非回环地址、额外服务、正式网络、正式数据目录和非预期端口。

也可直接查看渲染结果：

```bash
docker compose \
  --env-file .env.casdoor-test \
  -f compose.casdoor-test.yml \
  config
```

检查渲染结果时不得把包含密码的完整配置复制到聊天或日志。

## 启动

```bash
bash ./scripts/casdoor-test/start-casdoor-test.sh
```

脚本按顺序执行：

1. 校验私密环境和 Compose 隔离规则。
2. 启动 `postgres-test` 并等待 `healthy`。
3. 启动 `casdoor-test`。
4. 验证 volume、网络、数据库、回环端口和 OIDC Discovery。

PostgreSQL 不映射宿主机端口。Casdoor 只允许通过服务器自身的 `127.0.0.1:18000` 访问。

## 首次 Casdoor 配置

通过服务器本机浏览器或 SSH 隧道打开 Casdoor：

```bash
ssh -L 18000:127.0.0.1:18000 <test-server>
```

随后在开发电脑访问：

```text
http://127.0.0.1:18000
```

在 Casdoor 官方管理页面完成：

1. 按当前版本支持的流程初始化或登录测试管理员。
2. 立即为测试管理员设置独立密码，不复用正式账号密码。
3. 创建 Organization：`pomegranate-test`。
4. 创建 Application：`app-pomegranate-test`。
5. 只填写测试服务回调地址，不得填写正式 Account Server 回调。
6. 为 AI 助学等独立后端分别创建独立 Application 和 Client Secret。
7. 只创建测试用户，不复制正式 Casdoor 用户或数据。

当前正式 Account Server 只接受 `pomegranate` 和 `app-pomegranate`。不要修改正式 Account Server 来接受测试 Organization，也不要把它的 Casdoor URL 改为测试地址。

## 运行验证

```bash
bash ./scripts/casdoor-test/check-casdoor-test.sh
```

预期结果：

- `postgres-test` 为 `healthy`。
- `casdoor-test` 为 `running`。
- PostgreSQL 只使用 `pomegranate_casdoor_test_postgres_data`。
- 两个容器只连接 `pomegranate_casdoor_test_backend`。
- PostgreSQL 没有宿主机端口。
- Casdoor 只绑定 `127.0.0.1:18000`。
- `casdoor_test` 数据库和 `casdoor_test_app` 角色真实存在。
- OIDC Discovery issuer 为 `http://127.0.0.1:18000`。

Organization、Application、回调白名单和 Client Secret 必须在管理页面中人工核验，因为这些状态保存在测试数据库中，不属于 Git 配置。

## 停止和重新启动

停止但保留测试数据库：

```bash
docker compose \
  --env-file .env.casdoor-test \
  -f compose.casdoor-test.yml \
  stop
```

移除测试容器和测试网络但保留 named volume：

```bash
docker compose \
  --env-file .env.casdoor-test \
  -f compose.casdoor-test.yml \
  down
```

不得运行 `down -v`，除非测试环境负责人明确批准删除 `pomegranate_casdoor_test_postgres_data`。任何情况下都不得删除或挂载正式 Cloud PostgreSQL volume。

## 明确不包含

- 正式 Caddy 或测试公网入口
- 正式 Account Server
- 正式 PostgreSQL database 或 volume
- 正式 Casdoor 用户和配置
- 正式 Client Secret
- 真实密码或 `.env.casdoor-test`
- AI 助学业务接口

如未来必须提供公网 HTTPS，应使用独立服务器或独立公网入口重新评审；不得直接修改正式 Caddyfile 或让测试 HTTP 配置成为正式默认值。
