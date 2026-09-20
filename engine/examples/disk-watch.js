({async evaluate(ctx) {
  const sample = await ctx.read('disk');
  const {high = 10, low = 6, recovery = 12, maxAgeMs = 60000} = ctx.params;
  if (!(low < high && high < recovery)) throw Error('invalid disk thresholds');
  if (!sample.hasValue || sample.quality !== 'good' || !Number.isFinite(sample.value)
      || ctx.now() - sample.timestamp > maxAgeMs) return;
  if (sample.value >= recovery) {
    ctx.state.high = false;
    ctx.state.low = false;
    return;
  }
  if (sample.value < high) ctx.state.high = true;
  if (sample.value < low && !ctx.state.low) {
    ctx.state.high = true;
    ctx.state.low = true;
    await ctx.trigger('investigate');
  }
}})
