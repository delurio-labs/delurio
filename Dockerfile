# The same build as the npm package: dist/ from `npm run build`, the schemas check validates with, the production
# dependencies, and nothing else.
FROM node:26.13.1-alpine3.24@sha256:6bd9ab43c0ec0190ca7b2d193906cb8b7edf6e7f4afa43ddeccd6452710bffb5
WORKDIR /opt/delurio
COPY package.json package-lock.json ./
RUN npm ci --omit=dev --ignore-scripts && npm cache clean --force
COPY dist ./dist
COPY schemas ./schemas
COPY SPEC.md NOTICE LICENSE ./
WORKDIR /work
USER node
ENTRYPOINT ["node", "/opt/delurio/dist/cli.js"]
