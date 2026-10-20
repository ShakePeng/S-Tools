// Adapted from Kiri (MIT License), commit 52d2d2f8fd613ce3341cbfae371009f06b123903.
// Copyright (c) 2026 yuxino. See THIRD_PARTY_NOTICES.md.

/** Three separable box passes approximate a Gaussian, with clamped, premultiplied edges. */
export function blurRgba(data:Uint8ClampedArray,width:number,height:number,sigma:number):void {
  if(sigma<=0||!width||!height)return;
  let input=new Float32Array(data.length),output=new Float32Array(data.length);
  for(let i=0;i<data.length;i+=4){const alpha=data[i+3]/255;input[i]=data[i]*alpha;input[i+1]=data[i+1]*alpha;input[i+2]=data[i+2]*alpha;input[i+3]=data[i+3];}
  let lower=Math.floor(Math.sqrt(4*sigma*sigma+1));if(lower%2===0)lower--;
  lower=Math.max(1,lower);
  const count=Math.round((12*sigma*sigma-3*lower*lower-12*lower-9)/(-4*lower-4));
  for(let pass=0;pass<3;pass++){
    const radius=((pass<count?lower:lower+2)-1)/2;
    for(const horizontal of [true,false]){
      const length=horizontal?width:height,lines=horizontal?height:width,stride=horizontal?4:width*4,divisor=2*radius+1;
      for(let line=0;line<lines;line++){
        const base=horizontal?line*width*4:line*4;
        let r=0,g=0,b=0,a=0;
        for(let k=-radius;k<=radius;k++){const i=base+Math.max(0,Math.min(length-1,k))*stride;r+=input[i];g+=input[i+1];b+=input[i+2];a+=input[i+3];}
        for(let position=0;position<length;position++){
          const i=base+position*stride;output[i]=r/divisor;output[i+1]=g/divisor;output[i+2]=b/divisor;output[i+3]=a/divisor;
          const add=base+Math.min(length-1,position+radius+1)*stride,remove=base+Math.max(0,position-radius)*stride;
          r+=input[add]-input[remove];g+=input[add+1]-input[remove+1];b+=input[add+2]-input[remove+2];a+=input[add+3]-input[remove+3];
        }
      }
      [input,output]=[output,input];
    }
  }
  for(let i=0;i<data.length;i+=4){const alpha=input[i+3];data[i]=alpha>0?input[i]*255/alpha:0;data[i+1]=alpha>0?input[i+1]*255/alpha:0;data[i+2]=alpha>0?input[i+2]*255/alpha:0;data[i+3]=alpha;}
}

/** Bounded work surface keeps live video and large screenshot brushes responsive. */
export function blurCanvas(canvas:HTMLCanvasElement,sigma:number):void {
  const {width,height}=canvas;if(!width||!height||sigma<=0)return;
  const scale=Math.max(1,sigma/4,Math.max(width,height)/640);
  const work=document.createElement("canvas");work.width=Math.max(1,Math.round(width/scale));work.height=Math.max(1,Math.round(height/scale));
  const small=work.getContext("2d",{willReadFrequently:true}),target=canvas.getContext("2d");if(!small||!target)return;
  small.drawImage(canvas,0,0,work.width,work.height);
  const pixels=small.getImageData(0,0,work.width,work.height);blurRgba(pixels.data,work.width,work.height,sigma/scale);small.putImageData(pixels,0,0);
  target.clearRect(0,0,width,height);target.drawImage(work,0,0,width,height);
}
