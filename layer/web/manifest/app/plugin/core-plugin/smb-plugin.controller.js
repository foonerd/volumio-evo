/**
 * SMB server lists (extra shares + optional named users). Loaded after main bundle — registers on `volumio`.
 * Socket events: getSmbServerLists / pushSmbServerLists, saveSmbExtraShares, saveSmbUsers.
 */
(function () {
  'use strict';

  SmbPluginController.$inject = ['socketService', '$scope', '$log'];

  function SmbPluginController(socketService, $scope, $log) {
    var vm = this;
    vm.extraShares = [];
    vm.smbUsers = [];

    function onLists(data) {
      // socketService already wraps callbacks in $apply (see socket.service.js).
      vm.extraShares = angular.copy((data && data.extra_shares) || []);
      vm.smbUsers = angular.copy((data && data.smb_users) || []);
      angular.forEach(vm.smbUsers, function (row) {
        row.password = '';
      });
    }

    vm.addShareRow = function () {
      vm.extraShares.push({ name: '', path: '' });
    };

    vm.removeShareRow = function (idx) {
      vm.extraShares.splice(idx, 1);
    };

    vm.saveShares = function () {
      socketService.emit('saveSmbExtraShares', { extra_shares: vm.extraShares });
    };

    vm.addUserRow = function () {
      vm.smbUsers.push({ username: '', password: '' });
    };

    vm.removeUserRow = function (idx) {
      vm.smbUsers.splice(idx, 1);
    };

    vm.saveUsers = function () {
      var payload = vm.smbUsers.map(function (u) {
        var o = { username: (u.username || '').trim() };
        var pw = (u.password || '').trim();
        if (pw.length) {
          o.password = pw;
        }
        return o;
      });
      socketService.emit('saveSmbUsers', { smb_users: payload });
    };

    socketService.on('pushSmbServerLists', onLists);

    $scope.$on('$destroy', function () {
      // Remove all listeners for this event (inner callback is not the same ref as on()).
      socketService.off('pushSmbServerLists');
    });

    socketService.emit('getSmbServerLists');
    $log.debug('SmbPluginController: requested getSmbServerLists');
  }

  angular.module('volumio').controller('SmbPluginController', SmbPluginController);
})();
