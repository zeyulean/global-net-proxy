# aipro-wifi-router — 无线路由容器（hostapd + dnsmasq + sing-box tproxy）
# 架构：arm64（OrangePi AIpro），host 网络模式运行
FROM alpine:3.19

RUN apk add --no-cache hostapd dnsmasq iptables iproute2 bash curl \
    && mkdir -p /etc/hostapd /etc/dnsmasq /etc/sing-box

# sing-box 二进制从构建上下文放入（与 aipro gnp-proxy 同源，hy2/QUIC 出海）
COPY sing-box /usr/local/bin/sing-box
RUN chmod +x /usr/local/bin/sing-box

COPY rules/ /etc/sing-box/rules/
COPY hostapd.conf /etc/hostapd/hostapd.conf
COPY dnsmasq.conf /etc/dnsmasq/dnsmasq.conf
COPY sing-box-tproxy.json.template /etc/sing-box/config.template
COPY entrypoint.sh /entrypoint.sh
RUN chmod +x /entrypoint.sh

ENTRYPOINT ["/entrypoint.sh"]
