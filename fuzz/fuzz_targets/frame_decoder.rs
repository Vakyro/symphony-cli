#![no_main]

//! Frames IPC (STACK §6.2): cualquier tajo de bytes no puede hacer panic,
//! colgar el decodificador ni producir un frame mayor que `MAX_FRAME_LEN`.

use libfuzzer_sys::fuzz_target;
use symphony_protocol::{FrameDecoder, MAX_FRAME_LEN};

fuzz_target!(|data: &[u8]| {
    let mut dec = FrameDecoder::new();
    dec.push(data);
    // Alimentar de nuevo lo pendiente: el buffer interno nunca debería crecer
    // más allá de lo que la entrada anuncia con una cabecera válida.
    loop {
        match dec.next_frame() {
            Ok(Some(frame)) => assert!(
                frame.len() as u32 <= MAX_FRAME_LEN,
                "frame más grande que lo permitido"
            ),
            // Frame incompleto: queda pendiente, no se consume.
            Ok(None) => break,
            // Cabecera que anuncia más de MAX_FRAME_LEN: rechazada sin reservar.
            Err(_) => break,
        }
    }
});
