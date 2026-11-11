#!/usr/bin/env python3
"""Tiny WebDriver client for a headless QA run (see scripts/qa/README.md). Usage: wd.py CMD [ARGS]
 new | quit | shot NAME | js SCRIPT | ajs SCRIPT | click SEL | clickx XPATH | text SEL
 fill SEL TEXT (clear+type) | send SEL TEXT | key KEYNAME [MODS..] | rect W H | src
SEL is CSS; a leading 'x:' means XPath; 't:Label' means the first button/link/element whose own text is Label.
"""
import sys, json, base64, urllib.request, os, time
REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
D = os.environ.get('QA_OUT') or sys.exit('set QA_OUT to the run folder (outside the repository)')
BASE = 'http://127.0.0.1:4455'
SF = D + '/session.id'
KEYS = {'Enter': '', 'Tab': '', 'Shift': '', 'Control': '', 'Escape': '',
        'Backspace': '', 'PageDown': '', 'PageUp': '', 'End': '', 'Home': '',
        'Down': '', 'Up': '', 'Space': ' '}

def req(method, path, body=None, timeout=120):
    data = None if body is None else json.dumps(body).encode()
    r = urllib.request.Request(BASE + path, data=data, method=method, headers={'Content-Type': 'application/json'})
    try:
        with urllib.request.urlopen(r, timeout=timeout) as f:
            return json.loads(f.read())['value']
    except urllib.error.HTTPError as e:
        v = json.loads(e.read()).get('value', {})
        raise SystemExit('WD ERROR: %s: %s' % (v.get('error'), str(v.get('message'))[:400]))

def sid():
    return open(SF).read().strip()

def S(method, path, body=None, timeout=120):
    return req(method, '/session/' + sid() + path, body, timeout)

def find(sel):
    if sel.startswith('x:'):
        using, val = 'xpath', sel[2:]
    elif sel.startswith('t:'):
        t = sel[2:]
        using, val = 'xpath', "(//*[self::button or self::a or @role='button' or @role='link' or @role='tab'][normalize-space(.)=%s] | //*[normalize-space(text())=%s])[1]" % (json.dumps(t), json.dumps(t))
    else:
        using, val = 'css selector', sel
    el = S('POST', '/element', {'using': using, 'value': val})
    return list(el.values())[0]

def main():
    cmd, a = sys.argv[1], sys.argv[2:]
    if cmd == 'new':
        caps = {'capabilities': {'alwaysMatch': {'webkitgtk:browserOptions': {
            'binary': os.path.join(REPO, 'target', 'debug', 'threshold'), 'args': []}}}}
        v = req('POST', '/session', caps)
        open(SF, 'w').write(v['sessionId'])
        print('session', v['sessionId'])
    elif cmd == 'quit':
        print(S('DELETE', ''))
    elif cmd == 'shot':
        png = S('GET', '/screenshot')
        open('%s/%s.png' % (D, a[0]), 'wb').write(base64.b64decode(png))
    elif cmd == 'js':
        print(json.dumps(S('POST', '/execute/sync', {'script': a[0], 'args': a[1:]}), ensure_ascii=False, indent=1))
    elif cmd == 'ajs':
        print(json.dumps(S('POST', '/execute/async', {'script': a[0], 'args': a[1:]}, timeout=300), ensure_ascii=False, indent=1))
    elif cmd == 'click':
        S('POST', '/element/%s/click' % find(a[0]), {})
    elif cmd == 'text':
        print(S('GET', '/element/%s/text' % find(a[0])))
    elif cmd == 'fill':
        e = find(a[0]); S('POST', '/element/%s/clear' % e, {}); S('POST', '/element/%s/value' % e, {'text': a[1]})
    elif cmd == 'send':
        S('POST', '/element/%s/value' % find(a[0]), {'text': a[1]})
    elif cmd == 'key':
        k = KEYS.get(a[0], a[0]); mods = [KEYS[m] for m in a[1:]]
        acts = [{'type': 'keyDown', 'value': m} for m in mods] + [{'type': 'keyDown', 'value': k}, {'type': 'keyUp', 'value': k}] + [{'type': 'keyUp', 'value': m} for m in reversed(mods)]
        S('POST', '/actions', {'actions': [{'type': 'key', 'id': 'kb', 'actions': acts}]})
        S('DELETE', '/actions')
    elif cmd == 'rect':
        print(S('POST', '/window/rect', {'width': int(a[0]), 'height': int(a[1])}))
    elif cmd == 'src':
        print(S('GET', '/source'))
    elif cmd == 'timeouts':
        S('POST', '/timeouts', {'script': 300000})
    else:
        raise SystemExit(__doc__)

main()
