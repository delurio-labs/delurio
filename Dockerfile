# The same build as the npm package: dist/ from `npm run build`, the production dependencies, and nothing else.
FROM node:22-alpine
WORKDIR /opt/delurio
COPY package.json package-lock.json ./
RUN npm ci --omit=dev --ignore-scripts && npm cache clean --force
COPY dist ./dist
COPY SPEC.md NOTICE LICENSE ./
WORKDIR /work
USER node
ENTRYPOINT ["node", "/opt/delurio/dist/cli.js"]
