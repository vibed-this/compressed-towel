chcp 65001 >nul
@echo off
REM 输出 **必须 MUST**是 UTF-8（首行 chcp 65001 不可删除，启动器不做 GBK 猜测）。
REM 最小可运行示例：更新检查通过返回 0，需要中止启动返回非零。
REM 子进程 stdin 为 null，这里只做展示，不可等待用户输入。

echo [check_update] checking for updates...
echo [check_update] OK
exit /b 0
