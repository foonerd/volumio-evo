#!/usr/bin/env node
/**
 * Smoke-test SMB Socket.IO handlers without the Angular UI.
 *
 *   npm install socket.io-client
 *   node scripts/dev-smb-socket-smoke.js http://127.0.0.1:3000
 *
 * Prints pushSmbServerLists payload and exits 0.
 */
'use strict';

const url = process.argv[2] || process.env.VOLUMIO_EVO_SOCKET_URL || 'http://127.0.0.1:3000';

let io;
try {
  ({ io } = require('socket.io-client'));
} catch (_) {
  console.error('Missing dependency: npm install socket.io-client');
  process.exit(2);
}

const socket = io(url, { transports: ['websocket', 'polling'], timeout: 8000 });

socket.on('connect', () => {
  console.error('connected:', url);
  socket.emit('getSmbServerLists');
});

socket.on('pushSmbServerLists', (data) => {
  console.log(JSON.stringify(data, null, 2));
  socket.close();
  process.exit(0);
});

socket.on('connect_error', (err) => {
  console.error('connect_error:', err.message);
  process.exit(1);
});

setTimeout(() => {
  console.error('timeout waiting for pushSmbServerLists');
  process.exit(1);
}, 12000);
