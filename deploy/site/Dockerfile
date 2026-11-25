# OpenThor patch site: nginx serving the folder `otp publish` wrote. Mount it at /usr/share/nginx/html (read-only), or
# build it into the image with `COPY site/ /usr/share/nginx/html/`.
FROM nginx:1.27-alpine
COPY nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80
HEALTHCHECK --interval=30s --timeout=3s CMD wget -qO- http://127.0.0.1/healthz || exit 1
