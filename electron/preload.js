const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('dock', {
  onSnapshot(callback) {
    ipcRenderer.on('dock:snapshot', (_event, message) => callback(message));
  },
  ballClicked() { ipcRenderer.send('dock:ball-clicked'); },
  ackTask(taskId) { ipcRenderer.send('dock:ack-task', taskId); },
  closeList() { ipcRenderer.send('dock:list-close'); },
});
