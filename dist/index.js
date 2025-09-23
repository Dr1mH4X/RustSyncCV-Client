window.addEventListener('DOMContentLoaded', () => {
  const serverInput = document.getElementById('serverUrl');
  const tokenInput = document.getElementById('token');
  const connectBtn = document.getElementById('connectBtn');
  const statusSpan = document.getElementById('status');

  connectBtn.addEventListener('click', async () => {
    const server = serverInput.value;
    const token = tokenInput.value;
    statusSpan.innerText = '连接中...';
    try {
      const result = await window.__TAURI__.invoke('connect', { server, token });
      if (result.success) {
        statusSpan.innerText = '已连接';
      } else {
        statusSpan.innerText = '连接失败: ' + result.message;
      }
    } catch (e) {
      statusSpan.innerText = '错误: ' + e;
    }
  });

  // 监听后端状态更新事件
  window.__TAURI__.event.listen('status-changed', event => {
    const payload = event.payload;
    statusSpan.innerText = payload.message;
  });
  // 监听系统托盘操作事件
  window.__TAURI__.event.listen('tray-sync', () => {
    statusSpan.innerText = '手动同步中...';
    // TODO: 调用后端同步函数 invoke('sync')
  });
  window.__TAURI__.event.listen('tray-pause', () => {
    statusSpan.innerText = '已暂停同步';
  });
  window.__TAURI__.event.listen('tray-resume', () => {
    statusSpan.innerText = '已恢复同步';
  });
});