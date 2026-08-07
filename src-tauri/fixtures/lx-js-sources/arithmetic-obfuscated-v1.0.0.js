/*!
 * @name Arithmetic Obfuscated Test Source
 * @description Regression fixture for folded decoder offsets.
 * @version 1.0.0
 * @author Fika Music
 */

function _0xstrings() {
  const table = [
    'CMvXDwvZDa==',
    'BxvZAwnvCMW=',
    'Aw5PDgvK',
    'mti4AW==',
  ];
  _0xstrings = function () {
    return table;
  };
  return _0xstrings();
}

function _0xdecode(_0xindex) {
  _0xindex = _0xindex - (-0x805 * 0x2 + -0xf63 + -0x10f * -0x1f);
  const encoded = _0xstrings()[_0xindex];
  const alphabet = 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789+/=';
  let buffer = 0;
  let bitCount = 0;
  let bytes = '';

  for (const character of encoded) {
    const value = alphabet.indexOf(character);
    if (value < 0 || value === 64) break;
    buffer = (buffer << 6) | value;
    bitCount += 6;
    if (bitCount >= 8) {
      bitCount -= 8;
      bytes += String.fromCharCode((buffer >> bitCount) & 0xff);
      buffer &= (1 << bitCount) - 1;
    }
  }

  let escaped = '';
  for (let index = 0; index < bytes.length; index += 1) {
    escaped += `%${bytes.charCodeAt(index).toString(16).padStart(2, '0')}`;
  }
  return decodeURIComponent(escaped);
}

const { EVENT_NAMES, on, send } = globalThis['lx'];

on(EVENT_NAMES[_0xdecode('0x164')], ({ action }) => {
  if (action !== _0xdecode(0x165)) {
    return Promise.reject(new Error('unsupported action'));
  }
  return Promise.resolve('https://cdn.example.test/song.mp3');
});

send(EVENT_NAMES[_0xdecode(0x166)], {
  sources: {
    kg: {
      name: 'Kugou',
      type: 'music',
      actions: [_0xdecode(0x165)],
      qualitys: [_0xdecode(0x167)],
    },
  },
});
