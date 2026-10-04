{async run(ctx){
 const request=await ctx.source('trigger',{kind:'http',path:'/check',method:'POST'});
 if(!Number.isFinite(request.requested)||request.requested<=0)throw Error('Invalid probe request');
 for(let i=0;i<34;i++){
  await ctx.sleep(5000);
  const result=await ctx.source('trigger',{kind:'http',path:'/status'});
  if(result.checked>=request.requested){
   if(result.success!==true)throw Error('One or more repositories could not be checked');
   return;
  }
 }
 throw Error('Host did not complete the requested probe in time');
}}
