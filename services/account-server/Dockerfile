ARG NODE_IMAGE=node:22.13.1-bookworm-slim
ARG COREPACK_VERSION=0.34.0
ARG PNPM_VERSION=11.9.0

FROM ${NODE_IMAGE} AS tooling
ARG COREPACK_VERSION
ARG PNPM_VERSION
ENV COREPACK_HOME=/opt/corepack
ENV PATH="${COREPACK_HOME}:${PATH}"
WORKDIR /workspace

RUN npm install --global "corepack@${COREPACK_VERSION}" \
    && corepack enable \
    && corepack prepare "pnpm@${PNPM_VERSION}" --activate

FROM tooling AS prod-dependencies
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY services/account-server/package.json services/account-server/package.json
RUN pnpm install --frozen-lockfile --prod --filter @pomegranate/account-server...

FROM tooling AS build
COPY package.json pnpm-lock.yaml pnpm-workspace.yaml ./
COPY services/account-server/package.json services/account-server/package.json
RUN pnpm install --frozen-lockfile --filter @pomegranate/account-server...

COPY services/account-server services/account-server
RUN pnpm --filter @pomegranate/account-server build

FROM ${NODE_IMAGE} AS runtime
ENV NODE_ENV=production
WORKDIR /workspace/services/account-server

COPY --from=prod-dependencies --chown=node:node /workspace/node_modules /workspace/node_modules
COPY --from=prod-dependencies --chown=node:node /workspace/services/account-server/node_modules ./node_modules
COPY --from=build --chown=node:node /workspace/services/account-server/package.json ./package.json
COPY --from=build --chown=node:node /workspace/services/account-server/dist ./dist
COPY --from=build --chown=node:node /workspace/services/account-server/migrations ./migrations

USER node
EXPOSE 3010

HEALTHCHECK --interval=15s --timeout=5s --start-period=15s --retries=5 \
  CMD ["node", "-e", "fetch(`http://127.0.0.1:${process.env.ACCOUNT_SERVER_PORT || '3010'}/health/ready`).then((response) => { if (!response.ok) process.exit(1); }).catch(() => process.exit(1));"]

CMD ["node", "dist/src/index.js"]
