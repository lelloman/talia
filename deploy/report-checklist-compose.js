ctx => {
  const f = ctx.steps.facts.value;
  const analysis = ctx.steps.analysis;
  let assessments = {};
  if (analysis.status === 'succeeded') {
    try {
      const parsed = JSON.parse(analysis.value.summary);
      if (parsed && Array.isArray(parsed.assessments)) {
        for (const a of parsed.assessments) if (typeof a.id === 'string' && typeof a.assessment === 'string') {
          const note = a.assessment.replace(/\s+/g,' ').trim();
          if (note) assessments[a.id] = note.length > 350 ? note.slice(0,347)+'…' : note;
        }
      }
    } catch (_) { /* Invalid AI output never hides deterministic evidence. */ }
  }
  const icons = {OK:'✅', WARN:'⚠️', ERR:'❌'};
  const text = f.items.map(item => {
    if (item.state === 'OK') return '✅ '+item.name;
    const explanation = item.problems.map(p=>'   '+p.text).join('\n');
    const note = assessments[item.id] || 'Assessment unavailable; measured findings shown above.';
    return icons[item.state]+' '+item.name+' — '+item.state+'\n'+explanation+'\n   Assessment: '+note;
  }).join('\n');
  return {subject:f.state+' · Infrastructure report',summary:'',sections:[{title:'Checklist',text}]};
}
