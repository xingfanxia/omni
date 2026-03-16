FROM node:24-slim AS builder
WORKDIR /build

COPY sdk/typescript/ /sdk/typescript/
RUN cd /sdk/typescript && npm ci && npm run build

COPY connectors/linear/ /build/
RUN npm ci && npm run build

FROM node:24-slim
WORKDIR /app
COPY --from=builder /build/dist /app/dist
COPY --from=builder /build/node_modules /app/node_modules
ENV NODE_ENV=production
CMD ["node", "dist/main.js"]
