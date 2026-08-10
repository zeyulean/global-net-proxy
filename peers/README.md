# 预生成 peer 配置池目录
#
# 这个目录存储 server.sh --pre-gen 生成的 peer 配置包。
# 每个文件是一个 JSON，包含 client_id, wg_ip, private_key, public_key, status。
#
# 安全说明:
#   - 这些文件包含私钥，只应存在于 gitee 私有仓库
#   - register.sh 从这里拉取未使用的 peer
#   - server.sh --activate 从这里读取 peer 信息激活到 wg0
#
# 生成命令 (在 lwtop 上):
#   sudo bash server.sh --pre-gen 20
