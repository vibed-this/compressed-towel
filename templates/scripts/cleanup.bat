chcp 65001 >nul
@echo off
REM 输出 **必须 MUST**是 UTF-8（首行 chcp 65001 不可删除，启动器不做 GBK 猜测）。
REM 最小可运行示例：退出清理，返回 0 表示成功。
REM 子进程 stdin 为 null，这里只做展示，不可等待用户输入。

echo [cleanup] cleaning up...
echo [cleanup] OK
exit /b 0
