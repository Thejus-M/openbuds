import numpy as np, wave
SR=48000; D=38.0; BPM=120; B=60/BPM; n=int(SR*D); t=np.arange(n)/SR
out=np.zeros(n)
def add(sig,at,g=1.0):
    i=int(at*SR); e=min(n,i+len(sig)); out[i:e]+=g*sig[:e-i]
def env(d,a=0.005,r=None):
    m=int(d*SR); x=np.arange(m)/SR; e=np.minimum(1,x/a)*np.exp(-x/(r or d/4)); return x,e
hz=lambda m:440*2**((m-69)/12)
def kick():
    x,e=env(.35,.001,.08); f=50+120*np.exp(-x*30); return np.sin(2*np.pi*np.cumsum(f)/SR)*e
def hat():
    x,e=env(.05,.001,.012); return np.random.default_rng(1).normal(0,1,len(x))*e
def pluck(m,d=.3):
    x,e=env(d,.003,.09); f=hz(m); return (np.sin(2*np.pi*f*x)+.4*np.sin(4*np.pi*f*x)+.2*np.sign(np.sin(2*np.pi*f*x)))*e
def bass(m,d):
    x,e=env(d,.01,d*.6); f=hz(m); return (np.sin(2*np.pi*f*x)+.3*np.sin(4*np.pi*f*x))*e
def pad(ms,d):
    x,_=env(d); e=np.minimum(1,x/.4)*np.minimum(1,(d-x)/.4)
    return sum(np.sin(2*np.pi*hz(m)*x*(1+k*.003)) for m in ms for k in(-1,1))*e/len(ms)
prog=[(57,[69,72,76]),(53,[69,72,77]),(48,[67,72,76]),(55,[67,71,74])]  # Am F C G
bars=int(D/(4*B))+1
for bi in range(bars):
    bt=bi*4*B; root,ch=prog[bi%4]
    add(pad(ch,4*B),bt,.12)
    for k in range(8):
        add(bass(root-12, B/2),bt+k*B/2,.35 if bi>0 else .2)
        arp=ch+[ch[0]+12]
        add(pluck(arp[k%4]+12,.25),bt+k*B/2+(0 if k%2==0 else 0),.10 if bi>0 else .06)
    for k in range(4):
        if bt>=1.0: add(kick(),bt+k*B,.8)
    for k in range(8):
        add(hat(),bt+k*B/2+B/4,.08)
for c in [2.5,5,10.5,13.5,16.5,19,21.5,24.5,26.5,29,31,32.5,35.5]:
    add(pluck(81,.4),c,.12); add(pluck(88,.4),c,.06)
fade=np.minimum(1,(D-t)/1.2); out*=fade
out=np.tanh(out*1.1); out/=np.abs(out).max()*1.12
s=(np.stack([out,out],1)*32767).astype('<i2')
w=wave.open('music.wav','wb'); w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR); w.writeframes(s.tobytes()); w.close()
