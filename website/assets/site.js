(function(){
  var bar=document.querySelector('.top');
  function nav(){ bar.classList.toggle('scrolled', scrollY>8); }
  addEventListener('scroll',nav,{passive:true}); nav();
  var still =matchMedia('(prefers-reduced-motion: reduce)').matches;
  if(still) return;
  document.documentElement.classList.add('js');
  // window flattens as it scrolls into the middle of the screen
  var tilt=document.getElementById('tilt'), scene=document.getElementById('scene'), tick=false;
  function frame(){
    tick=false;
    var r=scene.getBoundingClientRect(), vh=innerHeight;
    var p=Math.min(1,Math.max(0,(vh*0.95-r.top)/(vh*0.75)));
    tilt.style.setProperty('--rx',(16*(1-p)).toFixed(2)+'deg');
    tilt.style.setProperty('--sc',(0.94+0.06*p).toFixed(3));
  }
  addEventListener('scroll',function(){ if(!tick){tick=true;requestAnimationFrame(frame);} },{passive:true});
  addEventListener('resize',frame); frame();
  // sections rise, numbers count, the agent log plays
  function count(el){
    var n=+el.dataset.n; if(!n) return;
    var t=el.firstChild, s=performance.now();
    (function step(now){ var k=Math.min(1,(now-s)/1100); t.nodeValue=Math.round(n*(1-Math.pow(1-k,3))); if(k<1) requestAnimationFrame(step); })(s);
  }
  function play(c){
    c.classList.add('run');
    [].forEach.call(c.querySelectorAll('.lines div'),function(d,i){ setTimeout(function(){d.classList.add('in');},350+i*420); });
  }
  if(!('IntersectionObserver' in window)){ document.documentElement.classList.remove('js'); return; }
  var io=new IntersectionObserver(function(es){
    es.forEach(function(e){
      if(!e.isIntersecting) return;
      e.target.classList.add('seen');
      var s=e.target.querySelector('strong[data-n]'); if(s) count(s);
      if(e.target.id==='console') play(e.target);
      io.unobserve(e.target);
    });
  },{threshold:.25});
  [].forEach.call(document.querySelectorAll('.rise'),function(el){io.observe(el);});
})();
// Language: remember a choice made with the switch, and offer the other
// language once to a browser set to it. Never redirect.
(function(){
  var KEY='regards-lang', hint=document.getElementById('suggest'), chosen=null;
  try{ chosen=localStorage.getItem(KEY); }catch(e){}
  [].forEach.call(document.querySelectorAll('.lang a, #suggest'),function(a){
    a.addEventListener('click',function(){ try{ localStorage.setItem(KEY,a.lang); }catch(e){} });
  });
  if(!hint || chosen) return;
  var wants=(navigator.language||'').slice(0,2).toLowerCase();
  if(wants===hint.lang && wants!==document.documentElement.lang) hint.hidden=false;
})();
