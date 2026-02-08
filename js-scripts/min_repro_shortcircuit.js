function side(){ console.log('SIDE CALLED'); throw new Error('side'); }
const g = { get b(){ console.log('GETTER CALLED'); return function(){ return { c: 99 }; } } };
const a_null = null;
const a_obj = { b(){ return this._b }, _b: { c: 42 } };
try { console.log('S1', a_null?.b(side()).c); } catch(e){ console.log('S1 err', e && e.message); }
try { console.log('S2', a_obj?.b(side()).c); } catch(e){ console.log('S2 err', e && e.message); }
try { console.log('S3', (a_obj?.b)().c); } catch(e){ console.log('S3 err', e && e.message); }
try { console.log('S4', a_obj.b?.().c); } catch(e){ console.log('S4 err', e && e.message); }
try { console.log('S5', (a_obj.b)?.().c); } catch(e){ console.log('S5 err', e && e.message); }
try { console.log('S6', a_obj?.b?.().c); } catch(e){ console.log('S6 err', e && e.message); }
try { console.log('S7', (a_obj?.b)?.().c); } catch(e){ console.log('S7 err', e && e.message); }
try { console.log('G1', g?.b()?.c); } catch(e){ console.log('G1 err', e && e.message); }
try { console.log('G2', g.b?.().c); } catch(e){ console.log('G2 err', e && e.message); }
